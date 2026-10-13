//! Incremental download from Garmin Connect into an inbox folder that `eenkah import` reads.
//!
//! How it stays fast and polite:
//! 1. no fixed sleep after each request; back-off only on 429/5xx (see `http::send`);
//! 2. "already have it?" is decided *before* a request is made;
//! 3. weight (and every other kind) resumes from the last day *downloaded*, not the last
//!    day that had a value, so a month without weigh-ins is not re-fetched forever;
//! 4. resting heart rate and weight are fetched as date ranges, not one request per day;
//! 5. hydration is not downloaded;
//! 6. there is no "last 24 h" import window: the inbox is imported by content hash.
//!
//! A day counts as final once it is more than `RECENT_DAYS` old; until then it is fetched
//! again on every sync, because Garmin keeps updating recent days (late sleep, uploads).

use crate::auth::{refresh_di, Tokens, DI_HEADERS};
use crate::http::{send, Counters, Req, Retry, Transport};
use anyhow::{bail, Context, Result};
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const RECENT_DAYS: i64 = 2;
pub const RANGE_DAYS: i64 = 90;
/// The HRV service's page size (python-garminconnect and garth both use 28).
pub const HRV_RANGE_DAYS: i64 = 28;
const ACTIVITY_PAGE: usize = 100;

/// Per-day data kinds fetched one day at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Daily {
    /// No longer downloaded: `/download-service/files/wellness/{day}` is Garmin's Health
    /// Snapshot download and answers 406 on days without a snapshot. HRV comes from
    /// `Ranged::Hrv`. Kept so download records written before the change still load.
    Wellness,
    Sleep,
    Summary,
}

/// Kinds the Connect API serves as date ranges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Ranged {
    Rhr,
    Weight,
    /// Overnight HRV with Garmin's baseline band and status.
    Hrv,
}

impl Ranged {
    /// Longest span one request may ask for.
    fn max_days(self) -> i64 {
        match self {
            Ranged::Hrv => HRV_RANGE_DAYS,
            _ => RANGE_DAYS,
        }
    }
}

/// What has already been downloaded. Kept next to the inbox; only final days are recorded.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub display_name: Option<String>,
    pub daily: BTreeMap<Daily, BTreeSet<NaiveDate>>,
    pub ranged: BTreeMap<Ranged, BTreeSet<NaiveDate>>,
    pub activities: BTreeSet<i64>,
    /// The activity list has been read completely back to this day. Asking for an earlier
    /// start (fetching older history) pages past activities that are already here.
    #[serde(default)]
    pub activities_since: Option<NaiveDate>,
}

impl State {
    pub fn load(path: &Path) -> Result<State> {
        match std::fs::read(path) {
            Ok(b) => Ok(serde_json::from_slice(&b).context("connect state is corrupt")?),
            Err(_) => Ok(State::default()),
        }
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}

/// What the sync may skip because the athlete's archive already has it (e.g. files imported
/// earlier from a folder or an export). Supplied by the store; keeps this crate free of SQL.
#[derive(Debug, Default)]
pub struct Known {
    pub daily: BTreeMap<Daily, BTreeSet<NaiveDate>>,
    pub ranged: BTreeMap<Ranged, BTreeSet<NaiveDate>>,
    pub activities: BTreeSet<i64>,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub start: NaiveDate,
    pub today: NaiveDate,
    /// Stop after this many API requests (a safety valve for the first, long backfill).
    pub max_requests: Option<u64>,
    /// Garmin display name of the signed-in account (`Client::profile`). Garmin builds it
    /// into the per-day URLs, and the download state belongs to it.
    pub account: String,
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub counters: Counters,
    pub files_written: usize,
    pub skipped_known: usize,
    pub activities_new: usize,
    pub days_fetched: BTreeMap<String, usize>,
    pub errors: Vec<String>,
    pub stopped_early: bool,
    /// The account the data came from (Garmin's display name, an opaque id).
    pub account: Option<String>,
    /// Days downloaded in this run where at least one reply held real data.
    pub days_with_data: usize,
    /// Days downloaded in this run where Garmin answered but every reply was empty. A whole
    /// run of these usually means the sign-in is not the account the watch syncs to.
    pub days_empty: usize,
}

/// A progress update from the download, for progress bars.
#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    /// "activities", "rhr", "weight" or "days".
    pub step: &'static str,
    pub done: u64,
    /// 0 while unknown.
    pub total: u64,
    /// What is being fetched right now, e.g. a date.
    pub item: String,
    pub counters: Counters,
    pub days_with_data: usize,
    pub days_empty: usize,
}

impl Progress {
    fn new(step: &'static str, done: u64, total: u64, item: impl Into<String>, rep: &Report, client: &Client) -> Progress {
        Progress {
            step,
            done,
            total,
            item: item.into(),
            counters: client.counters,
            days_with_data: rep.days_with_data,
            days_empty: rep.days_empty,
        }
    }
}

/// Whether a JSON reply has a non-null value at `pointer`, i.e. Garmin recorded something.
fn has_value(body: &[u8], pointer: &str) -> bool {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.pointer(pointer).cloned())
        .is_some_and(|v| !v.is_null())
}

/// A Garmin account: the opaque display name Garmin uses in URLs, and the person's name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: Option<String>,
}

impl Account {
    /// "Alfredo Deza (942e059d…)", or the id alone.
    pub fn describe(&self) -> String {
        let short: String = self.id.chars().take(8).collect();
        match &self.name {
            Some(n) => format!("{n} ({short}…)"),
            None => format!("{short}…"),
        }
    }
}

/// An authenticated API client. Renews the bearer token with its refresh token when needed
/// and writes the renewed tokens back.
pub struct Client<'a> {
    pub t: &'a dyn Transport,
    pub retry: Retry,
    pub tokens: Tokens,
    pub tokens_path: Option<PathBuf>,
    pub counters: Counters,
}

impl Client<'_> {
    /// `force` renews even a token that has not expired yet (after the API answered 401).
    fn bearer(&mut self, force: bool) -> Result<String> {
        if force || self.tokens.di.expires_at <= chrono::Utc::now().timestamp() + 120 {
            self.tokens.di = refresh_di(self.t, &self.retry, &mut self.counters, &self.tokens.domain, &self.tokens.di)?;
            if let Some(p) = &self.tokens_path {
                self.tokens.save(p)?;
            }
        }
        Ok(format!("Bearer {}", self.tokens.di.access_token))
    }

    fn request(&mut self, path_and_query: &str, force: bool) -> Result<crate::http::Resp> {
        let url = format!("https://connectapi.{}{}", self.tokens.domain, path_and_query);
        let auth = self.bearer(force)?;
        // the download service sends ZIP files and answers 406 to a JSON-only Accept
        let accept = if path_and_query.starts_with("/download-service/") { "*/*" } else { "application/json" };
        let r = DI_HEADERS.iter().fold(Req::get(url), |r, (k, v)| r.header(k, *v)).header("Accept", accept);
        send(self.t, &r.header("Authorization", auth), &self.retry, &mut self.counters)
    }

    /// GET an API path; `None` for 204/404 (no data that day).
    pub fn get(&mut self, path_and_query: &str) -> Result<Option<Vec<u8>>> {
        let mut r = self.request(path_and_query, false)?;
        if r.status == 401 {
            log::info!("Garmin answered 401 for {}; renewing the token and retrying once", crate::http::strip_query(path_and_query));
            r = self.request(path_and_query, true)?;
        }
        match r.status {
            200 => Ok(Some(r.body)),
            204 | 404 => Ok(None),
            401 | 403 => bail!(
                "Garmin refused the token (HTTP {}) for {}; run `eenkah login` again",
                r.status,
                crate::http::strip_query(path_and_query)
            ),
            s => bail!("HTTP {s} for {}", crate::http::strip_query(path_and_query)),
        }
    }

    /// Who the token belongs to; also proves the token works.
    pub fn profile(&mut self) -> Result<Account> {
        let b = self.get("/userprofile-service/socialProfile")?.ok_or_else(|| anyhow::anyhow!("Garmin returned no profile"))?;
        let v: serde_json::Value = serde_json::from_slice(&b).context("profile reply was not JSON")?;
        let id = v["displayName"].as_str().map(str::to_string).ok_or_else(|| anyhow::anyhow!("profile has no displayName"))?;
        let name = v["fullName"].as_str().or(v["userName"].as_str()).filter(|s| !s.trim().is_empty()).map(str::to_string);
        Ok(Account { id, name })
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// Extract every file of a ZIP into `dir`; returns how many were written.
fn unzip(bytes: &[u8], dir: &Path) -> Result<usize> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("download was not a ZIP")?;
    let mut n = 0;
    for i in 0..z.len() {
        let mut e = z.by_index(i)?;
        if e.is_dir() {
            continue;
        }
        let name = Path::new(e.name()).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let mut b = vec![];
        e.read_to_end(&mut b)?;
        write(&dir.join(name), &b)?;
        n += 1;
    }
    Ok(n)
}

/// Contiguous runs of days in `[start, end]` that are not in `done`, split into `max`-day chunks.
pub fn missing_ranges(done: &BTreeSet<NaiveDate>, start: NaiveDate, end: NaiveDate, max: i64) -> Vec<(NaiveDate, NaiveDate)> {
    let mut out = vec![];
    let mut cur: Option<(NaiveDate, NaiveDate)> = None;
    for d in start.iter_days().take_while(|d| *d <= end) {
        if done.contains(&d) {
            if let Some(r) = cur.take() {
                out.push(r);
            }
            continue;
        }
        cur = match cur {
            Some((a, _)) if (d - a).num_days() < max => Some((a, d)),
            Some(r) => {
                out.push(r);
                Some((d, d))
            }
            None => Some((d, d)),
        };
    }
    out.extend(cur);
    out
}

pub fn sync(client: &mut Client, inbox: &Path, known: &Known, opts: &Options, progress: &mut dyn FnMut(&Progress)) -> Result<Report> {
    std::fs::create_dir_all(inbox)?;
    // kept beside the inbox, not in it, so the file import never sees it
    let state_path = inbox.parent().map(|p| p.join("state.json")).unwrap_or_else(|| inbox.join("../state.json"));
    let mut st = State::load(&state_path)?;
    let mut rep = Report::default();
    let final_until = opts.today - Duration::days(RECENT_DAYS);
    let over_budget = |c: &Counters| opts.max_requests.is_some_and(|m| c.requests >= m);

    // the download state belongs to one account: another account's record of what is
    // already here would skip days this account never downloaded
    let dn = opts.account.clone();
    if st.display_name.as_deref() != Some(dn.as_str()) {
        if let Some(old) = &st.display_name {
            log::warn!("the download record belongs to another account ({old}); starting it over for {dn}");
        }
        st = State {
            display_name: Some(dn.clone()),
            ..Default::default()
        };
        st.save(&state_path)?;
    }
    rep.account = Some(dn.clone());

    // ---- activities: newest first, stop at the first page that holds nothing new, unless
    // the list has never been read this far back
    progress(&Progress::new("activities", 0, 0, "checking the activity list", &rep, client));
    let backfill = st.activities_since.is_none_or(|s| opts.start < s);
    let mut complete = false;
    let mut start = 0usize;
    'pages: loop {
        if over_budget(&client.counters) {
            rep.stopped_early = true;
            break;
        }
        let Some(b) = client.get(&format!("/activitylist-service/activities/search/activities?start={start}&limit={ACTIVITY_PAGE}"))? else {
            complete = true;
            break;
        };
        let list: Vec<serde_json::Value> = serde_json::from_slice(&b).context("activity list was not a JSON array")?;
        if list.is_empty() {
            complete = true;
            break;
        }
        let mut any_new = false;
        for a in &list {
            let Some(id) = a["activityId"].as_i64() else { continue };
            let begin = a["startTimeLocal"].as_str().and_then(|s| NaiveDate::parse_from_str(&s[..10.min(s.len())], "%Y-%m-%d").ok());
            if begin.is_some_and(|d| d < opts.start) {
                complete = true;
                break 'pages;
            }
            if st.activities.contains(&id) || known.activities.contains(&id) {
                rep.skipped_known += 1;
                continue;
            }
            any_new = true;
            if over_budget(&client.counters) {
                rep.stopped_early = true;
                break 'pages;
            }
            let dir = inbox.join("Activities");
            write(&dir.join(format!("activity_{id}.json")), serde_json::to_string(a)?.as_bytes())?; // no request: it is in the list
            match client.get(&format!("/download-service/files/activity/{id}")) {
                Ok(Some(zip)) => {
                    rep.files_written += 1 + unzip(&zip, &dir)?;
                    st.activities.insert(id);
                    rep.activities_new += 1;
                    let name = a["activityName"].as_str().unwrap_or("activity");
                    let when = a["startTimeLocal"].as_str().unwrap_or("");
                    progress(&Progress::new("activities", rep.activities_new as u64, 0, format!("{name} · {when}"), &rep, client));
                }
                Ok(None) => {
                    st.activities.insert(id); // manual entries have no file
                }
                Err(e) => rep.errors.push(format!("activity {id}: {e:#}")),
            }
        }
        st.save(&state_path)?;
        if list.len() < ACTIVITY_PAGE {
            complete = true;
            break;
        }
        if !any_new && !backfill {
            break;
        }
        start += ACTIVITY_PAGE;
    }
    if complete && !rep.stopped_early {
        st.activities_since = Some(st.activities_since.map_or(opts.start, |s| s.min(opts.start)));
        st.save(&state_path)?;
    }

    // ---- ranged kinds: one request per missing run of up to RANGE_DAYS days
    for kind in [Ranged::Hrv, Ranged::Rhr, Ranged::Weight] {
        let mut done = st.ranged.get(&kind).cloned().unwrap_or_default();
        done.extend(known.ranged.get(&kind).into_iter().flatten().filter(|d| **d <= final_until));
        let ranges = missing_ranges(&done, opts.start, opts.today, kind.max_days());
        let step = match kind {
            Ranged::Rhr => "rhr",
            Ranged::Weight => "weight",
            Ranged::Hrv => "hrv",
        };
        for (i, (a, b)) in ranges.iter().copied().enumerate() {
            if over_budget(&client.counters) {
                rep.stopped_early = true;
                break;
            }
            progress(&Progress::new(step, i as u64, ranges.len() as u64, format!("{a} → {b}"), &rep, client));
            let (path, file) = match kind {
                Ranged::Rhr => (
                    format!("/userstats-service/wellness/daily/{dn}?fromDate={a}&untilDate={b}&metricId=60"),
                    inbox.join(format!("RHR/rhr_{a}_{b}.json")),
                ),
                Ranged::Weight => (format!("/weight-service/weight/dateRange?startDate={a}&endDate={b}"), inbox.join(format!("Weight/weight_{a}_{b}.json"))),
                Ranged::Hrv => (format!("/hrv-service/hrv/daily/{a}/{b}"), inbox.join(format!("HRV/hrv_{a}_{b}.json"))),
            };
            match client.get(&path) {
                Ok(body) => {
                    if let Some(body) = body {
                        write(&file, &body)?;
                        rep.files_written += 1;
                    }
                    let e = st.ranged.entry(kind).or_default();
                    e.extend(a.iter_days().take_while(|d| *d <= b && *d <= final_until));
                    *rep.days_fetched.entry(format!("{kind:?}").to_lowercase()).or_default() += ((b - a).num_days() + 1) as usize;
                }
                Err(e) => rep.errors.push(format!("{kind:?} {a}..{b}: {e:#}")),
            }
        }
        st.save(&state_path)?;
    }

    // ---- per-day kinds, newest first so a capped first run still brings today's data
    let days: Vec<NaiveDate> = opts.start.iter_days().take_while(|d| *d <= opts.today).collect();
    for (k, d) in days.iter().rev().enumerate() {
        progress(&Progress::new("days", k as u64, days.len() as u64, d.to_string(), &rep, client));
        // (fetched anything this run, any reply held data)
        let mut day = (false, false);
        for kind in [Daily::Sleep, Daily::Summary] {
            let is_done = st.daily.get(&kind).is_some_and(|s| s.contains(d)) || (*d <= final_until && known.daily.get(&kind).is_some_and(|s| s.contains(d)));
            if is_done {
                rep.skipped_known += 1;
                continue;
            }
            if over_budget(&client.counters) {
                rep.stopped_early = true;
                st.save(&state_path)?;
                return Ok(finish(rep, client));
            }
            let year = inbox.join(format!("Monitoring/{}", &d.to_string()[..4]));
            // (files written, the reply held data)
            let res: Result<(usize, bool)> = match kind {
                Daily::Wellness => Ok((0, false)),
                Daily::Sleep => client
                    .get(&format!("/wellness-service/wellness/dailySleepData/{dn}?date={d}&nonSleepBufferMinutes=60"))
                    .and_then(|b| match b {
                        Some(b) => write(&inbox.join(format!("Sleep/sleep_{d}.json")), &b).map(|_| (1, has_value(&b, "/dailySleepDTO/sleepTimeSeconds"))),
                        None => Ok((0, false)),
                    }),
                Daily::Summary => client.get(&format!("/usersummary-service/usersummary/daily/{dn}?calendarDate={d}")).and_then(|b| match b {
                    Some(b) => write(&year.join(format!("daily_summary_{d}.json")), &b).map(|_| (1, has_value(&b, "/totalSteps") || has_value(&b, "/restingHeartRate"))),
                    None => Ok((0, false)),
                }),
            };
            match res {
                Ok((n, data)) => {
                    day = (true, day.1 || data);
                    rep.files_written += n;
                    if *d <= final_until {
                        st.daily.entry(kind).or_default().insert(*d);
                    }
                    *rep.days_fetched.entry(format!("{kind:?}").to_lowercase()).or_default() += 1;
                }
                Err(e) => rep.errors.push(format!("{kind:?} {d}: {e:#}")),
            }
        }
        match day {
            (true, true) => rep.days_with_data += 1,
            (true, false) => rep.days_empty += 1,
            _ => {}
        }
        if k % 10 == 9 {
            st.save(&state_path)?;
        }
    }
    progress(&Progress::new("days", days.len() as u64, days.len() as u64, "done", &rep, client));
    st.save(&state_path)?;
    Ok(finish(rep, client))
}

fn finish(mut rep: Report, client: &Client) -> Report {
    rep.counters = client.counters;
    rep
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::DiToken;
    use crate::http::fake::{ok, Fake};

    /// The inbox lives one level down so the sync state beside it stays inside the temp dir.
    struct Inbox(PathBuf);
    impl Inbox {
        fn path(&self) -> &Path {
            &self.0
        }
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn tokens() -> Tokens {
        Tokens {
            domain: "garmin.com".into(),
            di: DiToken {
                access_token: "A".into(),
                refresh_token: Some("R".into()),
                client_id: "C".into(),
                expires_at: i64::MAX,
            },
        }
    }

    fn zip_with(name: &str) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(vec![]);
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut z, b"FIT").unwrap();
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    fn connect() -> Fake {
        Fake::default()
            // like Garmin: the download service refuses a JSON-only Accept header
            .on(|r| {
                (r.url.contains("/download-service/") && r.headers.iter().any(|(k, v)| k == "Accept" && v == "application/json")).then(|| crate::http::Resp {
                    status: 406,
                    headers: vec![],
                    body: vec![],
                })
            })
            .on(|r| r.url.contains("socialProfile").then(|| ok(r#"{"displayName":"me"}"#)))
            .on(|r| {
                r.url
                    .contains("search/activities?start=0")
                    .then(|| ok(r#"[{"activityId":7,"startTimeLocal":"2026-10-08 09:00:00"},{"activityId":6,"startTimeLocal":"2026-10-01 09:00:00"}]"#))
            })
            .on(|r| r.url.contains("search/activities").then(|| ok("[]")))
            .on(|r| r.url.contains("/files/activity/").then(|| ok(zip_with("7_ACTIVITY.fit"))))
            .on(|r| {
                r.url
                    .contains("/hrv-service/hrv/daily/")
                    .then(|| ok(r#"{"hrvSummaries":[{"calendarDate":"2026-10-01","lastNightAvg":37}]}"#))
            })
            .on(|r| r.url.contains("dailySleepData").then(|| ok(r#"{"dailySleepDTO":{"sleepTimeSeconds":25200}}"#)))
            .on(|r| r.url.contains("usersummary/daily").then(|| ok("{}")))
            .on(|r| r.url.contains("userstats-service").then(|| ok("{}")))
            .on(|r| r.url.contains("weight/dateRange").then(|| ok("{}")))
    }

    fn opts() -> Options {
        Options {
            start: d("2026-10-01"),
            today: d("2026-10-10"),
            max_requests: None,
            account: "me".into(),
        }
    }

    fn no_sleep(_: std::time::Duration) {}

    #[test]
    fn download_rules_hold() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = tmp.path().join("inbox");
        let inbox = Inbox(inbox);
        let f = connect();
        let mut c = Client {
            t: &f,
            retry: Retry {
                sleep: no_sleep,
                ..Default::default()
            },
            tokens: tokens(),
            tokens_path: None,
            counters: Counters::default(),
        };
        // activity 6 was imported earlier (e.g. from a folder or an export)
        let known = Known {
            activities: [6].into(),
            ..Default::default()
        };
        let r = sync(&mut c, inbox.path(), &known, &opts(), &mut |_| {}).unwrap();
        let urls = f.urls();
        let count = |s: &str| urls.iter().filter(|u| u.contains(s)).count();
        assert_eq!(count("/files/activity/"), 1, "known activity is not downloaded again");
        assert_eq!(count("userstats-service"), 1, "RHR fetched as one range (rule 4)");
        assert_eq!(count("weight/dateRange"), 1, "weight fetched as one range (rule 4)");
        assert_eq!(count("hydration"), 0, "no hydration (rule 5)");
        assert_eq!(count("dailySleepData"), 10);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!((r.days_with_data, r.days_empty, r.account.as_deref()), (10, 0, Some("me")), "every night has sleep");
        assert_eq!(count("/hrv-service/hrv/daily/2026-10-01/2026-10-10"), 1, "HRV fetched as one range");
        assert_eq!(count("/files/wellness/"), 0, "no Health Snapshot downloads (they answer 406)");
        assert!(inbox.path().join("Activities/7_ACTIVITY.fit").exists());
        assert!(inbox.path().join("HRV/hrv_2026-10-01_2026-10-10.json").exists());
        assert!(inbox.path().join("Sleep/sleep_2026-10-05.json").exists());

        // second sync on the same day: only the recent (not yet final) days are asked again,
        // and the question is answered before any request (rule 2)
        let f2 = connect();
        let mut c2 = Client {
            t: &f2,
            retry: Retry {
                sleep: no_sleep,
                ..Default::default()
            },
            tokens: tokens(),
            tokens_path: None,
            counters: Counters::default(),
        };
        sync(&mut c2, inbox.path(), &known, &opts(), &mut |_| {}).unwrap();
        let urls = f2.urls();
        let count = |s: &str| urls.iter().filter(|u| u.contains(s)).count();
        assert_eq!(count("dailySleepData"), RECENT_DAYS as usize, "{urls:?}");
        assert_eq!(count("socialProfile"), 0, "display name cached");
        assert_eq!(count("/files/activity/"), 0);
        // weight resumes from the last day downloaded, not the last weigh-in (rule 3)
        assert!(urls.iter().any(|u| u.contains("weight/dateRange?startDate=2026-10-09&endDate=2026-10-10")), "{urls:?}");
    }

    #[test]
    fn known_days_from_an_earlier_archive_are_not_requested() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = tmp.path().join("inbox");
        let inbox = Inbox(inbox);
        let f = connect();
        let mut c = Client {
            t: &f,
            retry: Retry {
                sleep: no_sleep,
                ..Default::default()
            },
            tokens: tokens(),
            tokens_path: None,
            counters: Counters::default(),
        };
        let all: BTreeSet<NaiveDate> = d("2026-10-01").iter_days().take(10).collect();
        let known = Known {
            daily: [(Daily::Sleep, all.clone())].into(),
            ranged: [(Ranged::Rhr, all)].into(),
            activities: [6, 7].into(),
        };
        sync(&mut c, inbox.path(), &known, &opts(), &mut |_| {}).unwrap();
        let urls = f.urls();
        // the archive covers sleep up to 10-10, but 10-09 and 10-10 are recent → asked again
        assert_eq!(urls.iter().filter(|u| u.contains("dailySleepData")).count(), RECENT_DAYS as usize);
        assert!(urls.iter().any(|u| u.contains("fromDate=2026-10-09&untilDate=2026-10-10")), "{urls:?}");
    }

    #[test]
    fn request_budget_stops_cleanly_and_resumes() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = tmp.path().join("inbox");
        let inbox = Inbox(inbox);
        let f = connect();
        let mut c = Client {
            t: &f,
            retry: Retry {
                sleep: no_sleep,
                ..Default::default()
            },
            tokens: tokens(),
            tokens_path: None,
            counters: Counters::default(),
        };
        let mut o = opts();
        o.max_requests = Some(6);
        let r = sync(&mut c, inbox.path(), &Known::default(), &o, &mut |_| {}).unwrap();
        assert!(r.stopped_early);
        assert!(State::load(&inbox.path().parent().unwrap().join("state.json")).unwrap().display_name.is_some());
    }

    #[test]
    fn an_account_with_nothing_recorded_is_counted_and_progress_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = Inbox(tmp.path().join("inbox"));
        // what Garmin sends for an account without a watch: valid, empty replies
        let f = Fake::default()
            .on(|r| r.url.contains("socialProfile").then(|| ok(r#"{"displayName":"other"}"#)))
            .on(|r| r.url.contains("search/activities").then(|| ok("[]")))
            .on(|r| r.url.contains("dailySleepData").then(|| ok(r#"{"dailySleepDTO":{"sleepTimeSeconds":null}}"#)))
            .on(|r| r.url.contains("usersummary/daily").then(|| ok(r#"{"totalSteps":null,"calendarDate":"x"}"#)))
            .on(|r| r.url.contains("userstats-service").then(|| ok(r#"{"allMetrics":{"metricsMap":{}}}"#)))
            .on(|r| r.url.contains("weight/dateRange").then(|| ok(r#"{"dateWeightList":[]}"#)));
        let mut c = Client {
            t: &f,
            retry: Retry {
                sleep: no_sleep,
                ..Default::default()
            },
            tokens: tokens(),
            tokens_path: None,
            counters: Counters::default(),
        };
        let mut seen = vec![];
        let r = sync(&mut c, inbox.path(), &Known::default(), &opts(), &mut |p| seen.push((p.step, p.done, p.total))).unwrap();
        assert_eq!((r.days_with_data, r.days_empty), (0, 10));
        assert_eq!(seen.first(), Some(&("activities", 0, 0)));
        assert_eq!(seen.last(), Some(&("days", 10, 10)), "the last update says the download is complete");
        assert!(seen.contains(&("days", 0, 10)));
    }

    #[test]
    fn another_accounts_download_record_is_not_reused() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = Inbox(tmp.path().join("inbox"));
        let run = |account: &str| {
            let f = connect();
            let mut c = Client {
                t: &f,
                retry: Retry {
                    sleep: no_sleep,
                    ..Default::default()
                },
                tokens: tokens(),
                tokens_path: None,
                counters: Counters::default(),
            };
            let o = Options { account: account.into(), ..opts() };
            sync(&mut c, inbox.path(), &Known::default(), &o, &mut |_| {}).unwrap();
            f.urls()
        };
        run("old");
        let urls = run("new");
        let sleep: Vec<_> = urls.iter().filter(|u| u.contains("dailySleepData")).collect();
        assert_eq!(sleep.len(), 10, "every day is fetched again for the new account");
        assert!(sleep.iter().all(|u| u.contains("/dailySleepData/new?")), "{sleep:?}");
    }

    #[test]
    fn older_history_pages_past_activities_already_here() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = Inbox(tmp.path().join("inbox"));
        // page 1: 100 recent activities, all already here; page 2: one older activity
        let f = connect()
            .on(|r| {
                r.url.contains("search/activities?start=0&").then(|| {
                    let page: Vec<_> = (0..100).map(|i| serde_json::json!({"activityId": 1000 + i, "startTimeLocal": "2026-10-05 09:00:00"})).collect();
                    ok(serde_json::to_vec(&page).unwrap())
                })
            })
            .on(|r| {
                r.url
                    .contains("search/activities?start=100&")
                    .then(|| ok(r#"[{"activityId":5,"startTimeLocal":"2024-03-01 09:00:00"}]"#))
            });
        let mut rules = f.rules;
        rules.rotate_right(2); // the paging rules must win over connect()'s defaults
        let f = Fake { rules, ..Default::default() };
        let mut c = Client {
            t: &f,
            retry: Retry {
                sleep: no_sleep,
                ..Default::default()
            },
            tokens: tokens(),
            tokens_path: None,
            counters: Counters::default(),
        };
        let known = Known {
            activities: (1000..1100).collect(),
            ..Default::default()
        };
        let o = Options { start: d("2024-01-01"), ..opts() };
        let r = sync(&mut c, inbox.path(), &known, &o, &mut |_| {}).unwrap();
        assert_eq!(r.activities_new, 1, "the older activity behind a page of known ones is found");
        let st = State::load(&inbox.path().parent().unwrap().join("state.json")).unwrap();
        assert_eq!(st.activities_since, Some(d("2024-01-01")));
    }

    #[test]
    fn missing_ranges_by_hand() {
        let done: BTreeSet<NaiveDate> = [d("2026-01-03"), d("2026-01-04")].into();
        assert_eq!(
            missing_ranges(&done, d("2026-01-01"), d("2026-01-06"), 90),
            vec![(d("2026-01-01"), d("2026-01-02")), (d("2026-01-05"), d("2026-01-06"))]
        );
        assert_eq!(missing_ranges(&BTreeSet::new(), d("2026-01-01"), d("2026-01-05"), 2).len(), 3);
    }
}
