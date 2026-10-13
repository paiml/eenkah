//! HTTP with retries. No fixed sleeps between requests (FINDINGS §1 bug 1): a request is
//! repeated only after a 429 or 5xx, waiting `Retry-After` or an exponential back-off.
//!
//! Every request is logged at debug level (`eenkah -v`) and its headers at trace level
//! (`-vv`). Secrets never reach the log: bodies are not logged, and token-bearing query
//! values and headers are redacted.

use anyhow::{bail, Result};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

#[derive(Clone, Debug)]
pub struct Req {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Form body (application/x-www-form-urlencoded), POST only.
    pub form: Option<Vec<(String, String)>>,
    /// JSON body, POST only. Takes precedence over `form`.
    pub json: Option<String>,
}

impl Req {
    pub fn get(url: impl Into<String>) -> Req {
        Req {
            method: Method::Get,
            url: url.into(),
            headers: vec![],
            form: None,
            json: None,
        }
    }
    pub fn post(url: impl Into<String>, form: Vec<(String, String)>) -> Req {
        Req {
            method: Method::Post,
            url: url.into(),
            headers: vec![],
            form: Some(form),
            json: None,
        }
    }
    pub fn post_json(url: impl Into<String>, body: String) -> Req {
        Req {
            method: Method::Post,
            url: url.into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            form: None,
            json: Some(body),
        }
    }
    pub fn header(mut self, k: &str, v: impl Into<String>) -> Req {
        self.headers.push((k.to_string(), v.into()));
        self
    }
}

#[derive(Clone, Debug)]
pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
    pub fn header(&self, k: &str) -> Option<&str> {
        self.headers.iter().find(|(a, _)| a.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }
}

/// Anything that can send a request: the real network, or a fake in tests.
pub trait Transport: Send + Sync {
    fn send(&self, r: &Req) -> Result<Resp>;
}

/// The real network, with a cookie jar (the SSO login needs one).
pub struct Net {
    agent: ureq::Agent,
}

impl Net {
    pub fn new() -> Net {
        let cfg = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(60)))
            .max_redirects(10)
            .build();
        Net {
            agent: ureq::Agent::new_with_config(cfg),
        }
    }
}

impl Default for Net {
    fn default() -> Self {
        Net::new()
    }
}

impl Transport for Net {
    fn send(&self, r: &Req) -> Result<Resp> {
        let mut resp = match r.method {
            Method::Get => {
                let mut b = self.agent.get(&r.url);
                for (k, v) in &r.headers {
                    b = b.header(k, v);
                }
                b.call()?
            }
            Method::Post => {
                let mut b = self.agent.post(&r.url);
                for (k, v) in &r.headers {
                    b = b.header(k, v);
                }
                match &r.json {
                    Some(j) => b.send(j.as_str())?,
                    None => {
                        let form = r.form.clone().unwrap_or_default();
                        b.send_form(form.iter().map(|(k, v)| (k.as_str(), v.as_str())))?
                    }
                }
            }
        };
        let status = resp.status().as_u16();
        let headers = resp.headers().iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string())).collect();
        let body = resp.body_mut().with_config().limit(512 * 1024 * 1024).read_to_vec()?;
        Ok(Resp { status, headers, body })
    }
}

/// How retries wait; tests replace the sleeper so they run instantly.
pub struct Retry {
    pub attempts: u32,
    pub base: Duration,
    pub sleep: fn(Duration),
}

impl Default for Retry {
    fn default() -> Self {
        Retry {
            attempts: 6,
            base: Duration::from_secs(2),
            sleep: std::thread::sleep,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, serde::Serialize)]
pub struct Counters {
    pub requests: u64,
    pub retries: u64,
    pub bytes: u64,
}

/// Send exactly once, logging the exchange. Every status, 429 and 5xx included, comes back
/// as a response; only transport failures are errors.
pub fn send_once(t: &dyn Transport, r: &Req, c: &mut Counters) -> Result<Resp> {
    let method = if r.method == Method::Get { "GET" } else { "POST" };
    c.requests += 1;
    log::debug!("→ {method} {}", redact_url(&r.url));
    for (k, v) in &r.headers {
        log::trace!("    {k}: {}", redact_header(k, v));
    }
    let t0 = Instant::now();
    let out = t.send(r);
    let ms = t0.elapsed().as_millis();
    match &out {
        Ok(resp) => {
            c.bytes += resp.body.len() as u64;
            log::debug!("← {} {} · {} · {ms} ms{}", resp.status, resp.header("content-type").unwrap_or("-"), size(resp.body.len()), edge(resp));
            for (k, v) in &resp.headers {
                log::trace!("    {k}: {}", redact_header(k, v));
            }
        }
        Err(e) => log::debug!("✗ {method} {} failed after {ms} ms: {e:#}", strip_query(&r.url)),
    }
    out
}

/// Send with retries on 429 / 5xx / connection errors. Other statuses are returned as-is.
pub fn send(t: &dyn Transport, r: &Req, retry: &Retry, c: &mut Counters) -> Result<Resp> {
    let mut last_err = None;
    for attempt in 0..retry.attempts {
        if attempt > 0 {
            c.retries += 1;
        }
        let last = attempt + 1 == retry.attempts;
        match send_once(t, r, c) {
            Ok(resp) if resp.status == 429 || resp.status >= 500 => {
                let wait = resp
                    .header("retry-after")
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .map(Duration::from_secs)
                    .unwrap_or(retry.base * 2u32.pow(attempt))
                    .min(Duration::from_secs(300));
                last_err = Some(anyhow::anyhow!("HTTP {} from {}", resp.status, strip_query(&r.url)));
                if last {
                    break;
                }
                log::warn!(
                    "HTTP {} from {}; waiting {}s before retry {}/{}",
                    resp.status,
                    strip_query(&r.url),
                    wait.as_secs(),
                    attempt + 1,
                    retry.attempts - 1
                );
                (retry.sleep)(wait);
            }
            Ok(resp) => return Ok(resp),
            Err(e) => {
                last_err = Some(e);
                if last {
                    break;
                }
                let wait = retry.base * 2u32.pow(attempt);
                log::warn!("{} unreachable; waiting {}s before retry {}/{}", strip_query(&r.url), wait.as_secs(), attempt + 1, retry.attempts - 1);
                (retry.sleep)(wait);
            }
        }
    }
    match last_err {
        Some(e) => Err(e.context(format!("gave up after {} attempts", retry.attempts))),
        None => bail!("no attempts made"),
    }
}

pub fn strip_query(u: &str) -> &str {
    u.split('?').next().unwrap_or(u)
}

/// Query parameters whose values are credentials.
const SECRET_PARAMS: [&str; 6] = ["ticket", "service_ticket", "refresh_token", "access_token", "oauth_token", "password"];

/// The URL with credential-bearing query values replaced by `…`.
pub fn redact_url(u: &str) -> String {
    let Some((base, q)) = u.split_once('?') else { return u.to_string() };
    let q: Vec<String> = q
        .split('&')
        .map(|kv| match kv.split_once('=') {
            Some((k, _)) if SECRET_PARAMS.iter().any(|s| k.eq_ignore_ascii_case(s)) => format!("{k}=…"),
            _ => kv.to_string(),
        })
        .collect();
    format!("{base}?{}", q.join("&"))
}

fn redact_header(k: &str, v: &str) -> String {
    match k.to_ascii_lowercase().as_str() {
        "authorization" => format!("{} …", v.split(' ').next().unwrap_or("")),
        "cookie" | "set-cookie" => format!("{}=…", v.split('=').next().unwrap_or("")),
        _ => v.to_string(),
    }
}

fn size(n: usize) -> String {
    if n < 10_000 {
        format!("{n} B")
    } else {
        format!("{} kB", n / 1000)
    }
}

/// Markers that show who actually answered: Cloudflare's edge or Garmin's servers.
fn edge(r: &Resp) -> String {
    let mut s = String::new();
    if let Some(m) = r.header("cf-mitigated") {
        s += &format!(" · cf-mitigated: {m}");
    }
    if let Some(ray) = r.header("cf-ray") {
        s += &format!(" · cf-ray {ray}");
    }
    s
}

#[cfg(test)]
pub mod fake {
    //! A scripted transport: each request is answered by the first matching rule.
    use super::*;
    use std::sync::Mutex;

    type Rule = Box<dyn Fn(&Req) -> Option<Resp> + Send + Sync>;

    #[derive(Default)]
    pub struct Fake {
        pub rules: Vec<Rule>,
        pub log: Mutex<Vec<Req>>,
    }

    impl Fake {
        pub fn on(mut self, f: impl Fn(&Req) -> Option<Resp> + Send + Sync + 'static) -> Fake {
            self.rules.push(Box::new(f));
            self
        }
        pub fn urls(&self) -> Vec<String> {
            self.log.lock().unwrap().iter().map(|r| r.url.clone()).collect()
        }
    }

    pub fn ok(body: impl Into<Vec<u8>>) -> Resp {
        Resp {
            status: 200,
            headers: vec![],
            body: body.into(),
        }
    }

    impl Transport for Fake {
        fn send(&self, r: &Req) -> Result<Resp> {
            self.log.lock().unwrap().push(r.clone());
            for rule in &self.rules {
                if let Some(x) = rule(r) {
                    return Ok(x);
                }
            }
            Ok(Resp {
                status: 404,
                headers: vec![],
                body: b"no rule".to_vec(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn no_sleep(_: Duration) {}

    #[test]
    fn retries_429_then_succeeds_without_fixed_sleeps() {
        let n = std::sync::Arc::new(AtomicU32::new(0));
        let n2 = n.clone();
        let f = Fake::default().on(move |_| {
            let k = n2.fetch_add(1, Ordering::SeqCst);
            Some(if k < 2 {
                Resp {
                    status: 429,
                    headers: vec![("Retry-After".into(), "1".into())],
                    body: vec![],
                }
            } else {
                ok("fine")
            })
        });
        let mut c = Counters::default();
        let r = send(
            &f,
            &Req::get("https://x/y"),
            &Retry {
                sleep: no_sleep,
                ..Default::default()
            },
            &mut c,
        )
        .unwrap();
        assert_eq!((r.text().as_str(), c.requests, c.retries), ("fine", 3, 2));
    }

    #[test]
    fn gives_up_after_attempts() {
        let f = Fake::default().on(|_| {
            Some(Resp {
                status: 503,
                headers: vec![],
                body: vec![],
            })
        });
        let mut c = Counters::default();
        assert!(send(
            &f,
            &Req::get("https://x/y"),
            &Retry {
                attempts: 3,
                sleep: no_sleep,
                ..Default::default()
            },
            &mut c
        )
        .is_err());
        assert_eq!(c.requests, 3);
    }

    #[test]
    fn secrets_are_redacted_from_logged_urls() {
        assert_eq!(redact_url("https://x/p?ticket=ST-1&login-url=y&refresh_token=R"), "https://x/p?ticket=…&login-url=y&refresh_token=…");
        assert_eq!(redact_url("https://x/p"), "https://x/p");
        assert_eq!(redact_header("Authorization", "Bearer abc.def"), "Bearer …");
    }

    #[test]
    fn client_errors_are_returned_not_retried() {
        let f = Fake::default().on(|_| {
            Some(Resp {
                status: 401,
                headers: vec![],
                body: vec![],
            })
        });
        let mut c = Counters::default();
        assert_eq!(
            send(
                &f,
                &Req::get("https://x/y"),
                &Retry {
                    sleep: no_sleep,
                    ..Default::default()
                },
                &mut c
            )
            .unwrap()
            .status,
            401
        );
        assert_eq!(c.requests, 1);
    }
}
