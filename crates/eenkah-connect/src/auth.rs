//! Garmin Connect sign-in and tokens.
//!
//! Since March 2026 the old SSO web widget (HTML form + CSRF, then an OAuth1 ticket swap) no
//! longer reliably issues tickets. The flow the Connect app uses now, and the one
//! python-garminconnect (Sept 2026) follows:
//!
//! 1. POST email + password as JSON to `sso.garmin.com/mobile/api/login`. Garmin answers with a
//!    JSON verdict: `SUCCESSFUL` (with a service ticket), `MFA_REQUIRED`,
//!    `INVALID_USERNAME_PASSWORD`, `CAPTCHA_REQUIRED`, ...
//! 2. On `MFA_REQUIRED`, POST the code to `/mobile/api/mfa/verifyCode` (same cookies).
//! 3. Trade the ticket at `diauth.garmin.com` for a "DI" OAuth2 bearer token plus a refresh
//!    token. The refresh token renews the bearer without the password.
//!
//! Plain HTTP only: eenkah does not spoof TLS fingerprints, solve CAPTCHAs or rotate
//! identities to get past Garmin's bot protection. When that protection answers, the error
//! says so.
//!
//! The password is used once and never stored. Only tokens are saved, in a file readable by
//! the owner only.

use crate::http::{send, send_once, Counters, Req, Resp, Retry, Transport};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The Connect iOS app's SSO client, and the browser it signs in through.
pub const SSO_CLIENT_ID: &str = "GCM_IOS_DARK";
const SSO_UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148";
/// DI OAuth2 clients, newest first; Garmin retires old ones, so each is tried in turn.
pub const DI_CLIENT_IDS: [&str; 4] = [
    "GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2",
    "GARMIN_CONNECT_MOBILE_ANDROID_DI_2024Q4",
    "GARMIN_CONNECT_MOBILE_ANDROID_DI",
    "GARMIN_CONNECT_MOBILE_IOS_DI",
];
const DI_GRANT: &str = "https://connectapi.garmin.com/di-oauth2-service/oauth/grant/service_ticket";
/// Headers the Connect Android app sends with a DI bearer token.
pub const DI_HEADERS: [(&str, &str); 8] = [
    ("User-Agent", "GCM-Android-5.23"),
    (
        "X-Garmin-User-Agent",
        "com.garmin.android.apps.connectmobile/5.23; ; Google/sdk_gphone64_arm64/google; Android/33; Dalvik/2.1.0",
    ),
    ("X-Garmin-Paired-App-Version", "10861"),
    ("X-Garmin-Client-Platform", "Android"),
    ("X-App-Ver", "10861"),
    ("X-Lang", "en"),
    ("X-GCExperience", "GC5"),
    ("Accept-Language", "en-US,en;q=0.9"),
];

/// A DI OAuth2 token from the current sign-in flow.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DiToken {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// The DI client that issued it; refreshes must use the same one.
    pub client_id: String,
    /// Unix seconds.
    pub expires_at: i64,
}

/// The saved sign-in.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Tokens {
    pub domain: String,
    pub di: DiToken,
}

impl Tokens {
    pub fn load(path: &Path) -> Result<Tokens> {
        let s = std::fs::read_to_string(path).with_context(|| format!("not signed in to Garmin Connect (no {}); run `eenkah login`", path.display()))?;
        serde_json::from_str(&s).with_context(|| format!("the saved sign-in in {} is not one this version understands; run `eenkah login` again", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// RFC 3986 percent-encoding: everything except unreserved characters.
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn query(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&")
}

/// `alfredo@example.com` → `al…@example.com`, for logs.
fn mask_email(e: &str) -> String {
    match e.split_once('@') {
        Some((u, d)) => format!("{}…@{d}", u.chars().take(2).collect::<String>()),
        None => "…".into(),
    }
}

/// The first `n` characters of a body, on one line, for error messages.
fn snippet(b: &[u8], n: usize) -> String {
    let s: String = String::from_utf8_lossy(b).chars().take(n).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What the person at the keyboard is asked for during sign-in.
pub trait Prompt {
    fn mfa_code(&self) -> Result<String>;
}

/// Garmin's JSON verdict on a sign-in or MFA step.
struct Verdict {
    kind: String,
    message: String,
    ticket: Option<String>,
    mfa_method: Option<String>,
}

/// Read an SSO JSON reply, or explain who answered instead (rate limit, Cloudflare, an
/// outage), so the error says what actually happened.
fn verdict(r: &Resp, step: &str) -> Result<Verdict> {
    let ray = r.header("cf-ray").map(|x| format!(", cf-ray {x}")).unwrap_or_default();
    if r.status == 429 {
        let after = r.header("retry-after").map(|x| format!("; Garmin asks to wait {x}s")).unwrap_or_default();
        let body = snippet(&r.body, 160);
        let body = if body.is_empty() { String::new() } else { format!("; reply: {body:?}") };
        bail!(
            "Garmin rate-limited the {step} (HTTP 429{ray}{after}{body}). This is per network and per client; wait at least an \
             hour before trying again — retrying right away extends the block"
        );
    }
    let v: serde_json::Value = match serde_json::from_slice(&r.body) {
        Ok(v) => v,
        Err(_) => {
            let challenge = r.header("cf-mitigated").is_some() || (r.status == 403 && r.header("server").is_some_and(|s| s.eq_ignore_ascii_case("cloudflare")));
            if challenge {
                bail!(
                    "Garmin's bot protection (Cloudflare) answered the {step} instead of Garmin (HTTP {}{ray}). eenkah does not try \
                     to get past it. Try again later or from another network",
                    r.status
                );
            }
            bail!(
                "Garmin's {step} replied HTTP {} with {} instead of JSON: {:?}",
                r.status,
                r.header("content-type").unwrap_or("no content type"),
                snippet(&r.body, 160)
            );
        }
    };
    let s = |p: &str| v.pointer(p).and_then(|x| x.as_str()).map(str::to_string);
    if s("/error/status-code").as_deref() == Some("429") {
        bail!("Garmin rate-limited the {step} (429 in the reply{ray}); wait at least an hour before trying again");
    }
    let kind = s("/responseStatus/type").ok_or_else(|| anyhow!("Garmin's {step} reply (HTTP {}) has no responseStatus: {:?}", r.status, snippet(&r.body, 160)))?;
    Ok(Verdict {
        message: s("/responseStatus/message").unwrap_or_default(),
        ticket: s("/serviceTicketId"),
        mfa_method: s("/customerMfaInfo/mfaLastMethodUsed"),
        kind,
    })
}

/// Sign in with email and password; asks for an MFA code if the account requires one.
pub fn login(t: &dyn Transport, retry: &Retry, c: &mut Counters, domain: &str, email: &str, password: &str, prompt: &dyn Prompt) -> Result<Tokens> {
    let sso = format!("https://sso.{domain}");
    let service = format!("https://mobile.integration.{domain}/gcm/ios");
    let q = query(&[("clientId", SSO_CLIENT_ID), ("locale", "en-US"), ("service", &service)]);
    // Credentials are sent once (`send_once`), never retried: repeating a sign-in after a 429
    // or an outage only deepens a block or counts towards an account lock.
    let sso_post = |url: String, body: serde_json::Value| {
        Req::post_json(url, body.to_string())
            .header("User-Agent", SSO_UA)
            .header("Accept", "application/json, text/plain, */*")
            .header("Origin", &sso)
    };

    log::info!("1/3 signing in as {} at {sso}/mobile/api/login", mask_email(email));
    let body = serde_json::json!({"username": email, "password": password, "rememberMe": true, "captchaToken": ""});
    let r = send_once(t, &sso_post(format!("{sso}/mobile/api/login?{q}"), body), c).context("could not reach Garmin's sign-in service")?;
    let mut v = verdict(&r, "sign-in")?;
    log::info!("    Garmin says: {} ({}), HTTP {}", v.kind, v.message, r.status);

    if v.kind == "MFA_REQUIRED" {
        let method = v.mfa_method.clone().unwrap_or_else(|| "email".into());
        log::info!("2/3 Garmin sent a verification code by {method}");
        let code = prompt.mfa_code()?;
        let body = serde_json::json!({
            "mfaMethod": method,
            "mfaVerificationCode": code.trim(),
            "rememberMyBrowser": true,
            "reconsentList": [],
            "mfaSetup": false,
        });
        let r = send_once(t, &sso_post(format!("{sso}/mobile/api/mfa/verifyCode?{q}"), body), c).context("could not reach Garmin's MFA service")?;
        v = verdict(&r, "MFA verification")?;
        log::info!("    Garmin says: {} ({}), HTTP {}", v.kind, v.message, r.status);
    } else if v.kind == "SUCCESSFUL" {
        log::info!("2/3 no verification code needed");
    }

    let ticket = match v.kind.as_str() {
        "SUCCESSFUL" => v.ticket.ok_or_else(|| anyhow!("Garmin accepted the sign-in but sent no service ticket"))?,
        "INVALID_USERNAME_PASSWORD" => bail!("Garmin rejected the email or password (INVALID_USERNAME_PASSWORD). Several failures in a row can lock the account for a while"),
        "MFA_REQUIRED" => bail!("Garmin rejected the verification code (still MFA_REQUIRED)"),
        "CAPTCHA_REQUIRED" => bail!(
            "Garmin wants a CAPTCHA for this sign-in (CAPTCHA_REQUIRED), usually after failed or rapid attempts from this network. \
             eenkah does not solve CAPTCHAs: sign in once at https://connect.{domain} in a browser, wait a while, then retry"
        ),
        other => bail!("Garmin refused the sign-in: {other} ({})", v.message),
    };
    log::info!("3/3 trading the service ticket for an OAuth2 token at diauth.{domain}");
    let di = di_token(t, retry, c, domain, &ticket, &service)?;
    log::info!(
        "    got a token from {} (expires {})",
        di.client_id,
        chrono::DateTime::from_timestamp(di.expires_at, 0).map(|d| d.to_rfc3339()).unwrap_or_default()
    );
    Ok(Tokens { domain: domain.into(), di })
}

/// The `exp` and `client_id` claims of a JWT (unverified: Garmin holds the key).
fn jwt_claims(token: &str) -> (Option<i64>, Option<String>) {
    use base64::Engine;
    let payload = token
        .split('.')
        .nth(1)
        .and_then(|p| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(p.trim_end_matches('=')).ok());
    let v: Option<serde_json::Value> = payload.and_then(|b| serde_json::from_slice(&b).ok());
    match v {
        Some(v) => (v["exp"].as_i64(), v["client_id"].as_str().map(str::to_string)),
        None => (None, None),
    }
}

fn di_post(domain: &str, client_id: &str, form: Vec<(String, String)>) -> Req {
    use base64::Engine;
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("{client_id}:"));
    DI_HEADERS
        .iter()
        .fold(Req::post(format!("https://diauth.{domain}/di-oauth2-service/oauth/token"), form), |r, (k, v)| r.header(k, *v))
        .header("Authorization", format!("Basic {basic}"))
        .header("Accept", "application/json")
        .header("Cache-Control", "no-cache")
}

fn parse_di(r: &Resp, client_id: &str, old_refresh: Option<&str>) -> Result<DiToken> {
    #[derive(Deserialize)]
    struct Reply {
        access_token: String,
        refresh_token: Option<String>,
        expires_in: Option<i64>,
    }
    let x: Reply = serde_json::from_slice(&r.body).context("DI token reply was not the expected JSON")?;
    let (exp, cid) = jwt_claims(&x.access_token);
    Ok(DiToken {
        expires_at: exp.or(x.expires_in.map(|s| now() + s)).unwrap_or(now() + 3600),
        client_id: cid.unwrap_or_else(|| client_id.to_string()),
        refresh_token: x.refresh_token.or(old_refresh.map(str::to_string)),
        access_token: x.access_token,
    })
}

/// Trade an SSO service ticket for a DI token, trying each DI client in turn.
fn di_token(t: &dyn Transport, retry: &Retry, c: &mut Counters, domain: &str, ticket: &str, service: &str) -> Result<DiToken> {
    let mut refused = vec![];
    for id in DI_CLIENT_IDS {
        let form = vec![
            ("client_id".to_string(), id.to_string()),
            ("service_ticket".to_string(), ticket.to_string()),
            ("grant_type".to_string(), DI_GRANT.to_string()),
            ("service_url".to_string(), service.to_string()),
        ];
        let r = send(t, &di_post(domain, id, form), retry, c)?;
        if r.status == 200 {
            return parse_di(&r, id, None);
        }
        log::info!("    {id} refused: HTTP {} {:?}", r.status, snippet(&r.body, 160));
        refused.push(format!("{id}: HTTP {}", r.status));
    }
    bail!(
        "Garmin accepted the password but every DI client refused the ticket ({}); run with -v for the replies",
        refused.join(", ")
    )
}

/// Renew a DI token with its refresh token. No password needed.
pub fn refresh_di(t: &dyn Transport, retry: &Retry, c: &mut Counters, domain: &str, di: &DiToken) -> Result<DiToken> {
    let refresh = di
        .refresh_token
        .as_deref()
        .ok_or_else(|| anyhow!("the Garmin token has expired and cannot be renewed; run `eenkah login` again"))?;
    log::info!("renewing the Garmin token ({})", di.client_id);
    let form = vec![
        ("grant_type".to_string(), "refresh_token".to_string()),
        ("client_id".to_string(), di.client_id.clone()),
        ("refresh_token".to_string(), refresh.to_string()),
    ];
    let r = send(t, &di_post(domain, &di.client_id, form), retry, c)?;
    if r.status != 200 {
        bail!("Garmin would not renew the token (HTTP {} {:?}); run `eenkah login` again", r.status, snippet(&r.body, 160));
    }
    parse_di(&r, &di.client_id, Some(refresh))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::fake::{ok, Fake};

    struct Code;
    impl Prompt for Code {
        fn mfa_code(&self) -> Result<String> {
            Ok(" 123456\n".into())
        }
    }

    fn status(code: u16, body: &str) -> Resp {
        Resp {
            status: code,
            headers: vec![("content-type".into(), "application/json".into())],
            body: body.as_bytes().to_vec(),
        }
    }

    fn jwt(claims: &str) -> String {
        use base64::Engine;
        let e = |s: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s);
        format!("{}.{}.sig", e(r#"{"alg":"RS256"}"#), e(claims))
    }

    const OK: &str = r#"{"serviceTicketId":"ST-1-abc","responseStatus":{"type":"SUCCESSFUL","message":""}}"#;

    /// Garmin as of 2026: the first DI client is retired, the second issues tokens.
    fn garmin(first_reply: &'static str) -> Fake {
        let token = format!(
            r#"{{"access_token":"{}","refresh_token":"R1","expires_in":3600}}"#,
            jwt(r#"{"exp":2000000000,"client_id":"GARMIN_CONNECT_MOBILE_ANDROID_DI_2024Q4"}"#)
        );
        Fake::default()
            .on(move |r| r.url.contains("/mobile/api/login?").then(|| status(200, first_reply)))
            .on(|r| {
                r.url
                    .contains("/mobile/api/mfa/verifyCode?")
                    .then(|| status(200, r#"{"serviceTicketId":"ST-2-mfa","responseStatus":{"type":"SUCCESSFUL"}}"#))
            })
            .on(move |r| {
                r.url.contains("diauth.garmin.com").then(|| {
                    let form = r.form.as_ref().unwrap();
                    if form.contains(&("client_id".into(), DI_CLIENT_IDS[0].into())) {
                        status(400, r#"{"error":"invalid_client"}"#)
                    } else {
                        ok(token.clone())
                    }
                })
            })
    }

    fn no_sleep(_: std::time::Duration) {}
    fn retry() -> Retry {
        Retry {
            sleep: no_sleep,
            ..Default::default()
        }
    }

    #[test]
    fn full_login_without_mfa() {
        let f = garmin(OK);
        let t = login(&f, &retry(), &mut Counters::default(), "garmin.com", "me@example.com", "pw", &Code).unwrap();
        let di = t.di;
        assert_eq!(
            (di.client_id.as_str(), di.refresh_token.as_deref(), di.expires_at),
            ("GARMIN_CONNECT_MOBILE_ANDROID_DI_2024Q4", Some("R1"), 2000000000)
        );
        let log = f.log.lock().unwrap();
        let body: serde_json::Value = serde_json::from_str(log[0].json.as_deref().unwrap()).unwrap();
        assert_eq!((body["username"].as_str(), body["password"].as_str()), (Some("me@example.com"), Some("pw")));
        assert!(log[0].url.contains("clientId=GCM_IOS_DARK"));
        let di_req = log.last().unwrap();
        assert!(di_req.form.as_ref().unwrap().contains(&("service_ticket".into(), "ST-1-abc".into())));
        assert!(di_req.headers.iter().any(|(k, v)| k == "Authorization" && v.starts_with("Basic ")));
    }

    #[test]
    fn mfa_code_is_asked_for_and_sent() {
        let f = garmin(r#"{"responseStatus":{"type":"MFA_REQUIRED"},"customerMfaInfo":{"mfaLastMethodUsed":"email"}}"#);
        login(&f, &retry(), &mut Counters::default(), "garmin.com", "me@example.com", "pw", &Code).unwrap();
        let log = f.log.lock().unwrap();
        let m = log.iter().find(|r| r.url.contains("verifyCode")).unwrap();
        let body: serde_json::Value = serde_json::from_str(m.json.as_deref().unwrap()).unwrap();
        assert_eq!((body["mfaVerificationCode"].as_str(), body["mfaMethod"].as_str()), (Some("123456"), Some("email")));
        assert!(log.iter().any(|r| r.form.as_ref().is_some_and(|f| f.contains(&("service_ticket".into(), "ST-2-mfa".into())))));
    }

    #[test]
    fn wrong_password_is_a_clear_error() {
        let f = Fake::default().on(|_| {
            Some(status(
                401,
                r#"{"responseStatus":{"type":"INVALID_USERNAME_PASSWORD","message":"generalLoginInvalidUsernameOrPassword"}}"#,
            ))
        });
        let e = login(&f, &retry(), &mut Counters::default(), "garmin.com", "a", "b", &Code).unwrap_err();
        assert!(format!("{e}").contains("rejected the email or password"), "{e}");
    }

    #[test]
    fn rate_limit_is_explained_and_credentials_are_not_resent() {
        let f = Fake::default().on(|_| Some(status(429, "")));
        let e = login(&f, &retry(), &mut Counters::default(), "garmin.com", "a", "b", &Code).unwrap_err();
        assert!(format!("{e:#}").contains("rate-limited"), "{e:#}");
        assert_eq!(f.urls().len(), 1);
    }

    #[test]
    fn cloudflare_challenge_is_named() {
        let f = Fake::default().on(|_| {
            Some(Resp {
                status: 403,
                headers: vec![("cf-mitigated".into(), "challenge".into()), ("cf-ray".into(), "abc-IAD".into())],
                body: b"<html>Just a moment...</html>".to_vec(),
            })
        });
        let e = login(&f, &retry(), &mut Counters::default(), "garmin.com", "a", "b", &Code).unwrap_err();
        let e = format!("{e:#}");
        assert!(e.contains("Cloudflare") && e.contains("abc-IAD"), "{e}");
    }

    #[test]
    fn captcha_is_not_solved() {
        let f = Fake::default().on(|_| Some(status(200, r#"{"responseStatus":{"type":"CAPTCHA_REQUIRED"}}"#)));
        let e = login(&f, &retry(), &mut Counters::default(), "garmin.com", "a", "b", &Code).unwrap_err();
        assert!(format!("{e}").contains("CAPTCHA_REQUIRED"));
    }

    #[test]
    fn di_token_is_renewed_with_its_own_client() {
        let f = Fake::default().on(|r| r.url.contains("diauth").then(|| ok(r#"{"access_token":"A2","expires_in":60}"#)));
        let old = DiToken {
            access_token: "A1".into(),
            refresh_token: Some("R1".into()),
            client_id: "C".into(),
            expires_at: 0,
        };
        let new = refresh_di(&f, &retry(), &mut Counters::default(), "garmin.com", &old).unwrap();
        assert_eq!((new.access_token.as_str(), new.refresh_token.as_deref(), new.client_id.as_str()), ("A2", Some("R1"), "C"));
        let log = f.log.lock().unwrap();
        assert!(log[0].form.as_ref().unwrap().contains(&("grant_type".into(), "refresh_token".into())));
    }

    #[test]
    fn unknown_token_files_ask_for_a_new_sign_in() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("tokens.json");
        std::fs::write(&p, r#"{"domain":"garmin.com","oauth2":null}"#).unwrap();
        let e = Tokens::load(&p).unwrap_err();
        assert!(format!("{e:#}").contains("run `eenkah login` again"), "{e:#}");
    }

    #[test]
    fn tokens_file_is_private() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("connect/tokens.json");
        let t = Tokens {
            domain: "garmin.com".into(),
            di: DiToken {
                access_token: "a".into(),
                refresh_token: Some("r".into()),
                client_id: "c".into(),
                expires_at: 1,
            },
        };
        t.save(&p).unwrap();
        assert_eq!(Tokens::load(&p).unwrap(), t);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}
