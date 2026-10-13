//! Time handling: zones, calendar-aware buckets, durations and the
//! `from`/`until` expression language (Graphite and Grafana flavours).

use anyhow::{anyhow, bail, Result};
use chrono::{
    Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeZone, Timelike,
};
use chrono_tz::Tz;
use std::fmt;

/// A timezone: the machine's local zone or a named IANA zone.
#[derive(Clone, Copy, Debug)]
pub enum Zone {
    Local,
    Named(Tz),
}

impl Zone {
    pub fn parse(s: &str) -> Result<Zone> {
        let s = s.trim();
        if s.is_empty() || s.eq_ignore_ascii_case("local") {
            return Ok(Zone::Local);
        }
        if s.eq_ignore_ascii_case("utc") {
            return Ok(Zone::Named(chrono_tz::UTC));
        }
        s.parse::<Tz>()
            .map(Zone::Named)
            .map_err(|_| anyhow!("unknown timezone '{s}'"))
    }

    pub fn name(&self) -> String {
        match self {
            Zone::Local => iana_time_zone::get_timezone().unwrap_or_else(|_| "local".into()),
            Zone::Named(tz) => tz.name().to_string(),
        }
    }

    pub fn to_local(&self, ts: i64) -> NaiveDateTime {
        match self {
            Zone::Local => Local.timestamp_opt(ts, 0).single().map(|d| d.naive_local()),
            Zone::Named(tz) => tz.timestamp_opt(ts, 0).single().map(|d| d.naive_local()),
        }
        .unwrap_or_else(|| {
            chrono::DateTime::from_timestamp(ts, 0)
                .unwrap_or_default()
                .naive_utc()
        })
    }

    /// Wall-clock time to epoch seconds. Times that do not exist (DST gap)
    /// are pushed forward; ambiguous ones take the earlier instant.
    pub fn from_local(&self, n: NaiveDateTime) -> i64 {
        for bump in 0..3 {
            let t = n + Duration::hours(bump);
            let r = match self {
                Zone::Local => Local
                    .from_local_datetime(&t)
                    .earliest()
                    .map(|d| d.timestamp()),
                Zone::Named(tz) => tz.from_local_datetime(&t).earliest().map(|d| d.timestamp()),
            };
            if let Some(v) = r {
                return v;
            }
        }
        n.and_utc().timestamp()
    }

    pub fn offset_secs(&self, ts: i64) -> i64 {
        match self {
            Zone::Local => Local
                .timestamp_opt(ts, 0)
                .single()
                .map(|d| d.offset().fix().local_minus_utc() as i64),
            Zone::Named(tz) => tz
                .timestamp_opt(ts, 0)
                .single()
                .map(|d| d.offset().fix().local_minus_utc() as i64),
        }
        .unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Unit {
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

impl Unit {
    pub fn secs(self) -> i64 {
        match self {
            Unit::Second => 1,
            Unit::Minute => 60,
            Unit::Hour => 3600,
            Unit::Day => 86400,
            Unit::Week => 7 * 86400,
            Unit::Month => 30 * 86400,
            Unit::Year => 365 * 86400,
        }
    }
    fn suffix(self) -> &'static str {
        match self {
            Unit::Second => "s",
            Unit::Minute => "min",
            Unit::Hour => "h",
            Unit::Day => "d",
            Unit::Week => "w",
            Unit::Month => "mon",
            Unit::Year => "y",
        }
    }
    /// Accepts Graphite (`min`, `mon`) and Grafana (`m`, `M`) spellings.
    pub fn parse(s: &str) -> Option<Unit> {
        Some(match s {
            "M" => Unit::Month,
            _ => match s.to_ascii_lowercase().as_str() {
                "s" | "sec" | "secs" | "second" | "seconds" => Unit::Second,
                "m" | "min" | "mins" | "minute" | "minutes" => Unit::Minute,
                "h" | "hr" | "hrs" | "hour" | "hours" => Unit::Hour,
                "d" | "day" | "days" => Unit::Day,
                "w" | "wk" | "week" | "weeks" => Unit::Week,
                "mon" | "mo" | "month" | "months" => Unit::Month,
                "y" | "yr" | "year" | "years" => Unit::Year,
                _ => return None,
            },
        })
    }
}

/// An amount of calendar time, e.g. `7d`, `1mon`, `-1y`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub n: i64,
    pub unit: Unit,
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.n, self.unit.suffix())
    }
}

impl Span {
    pub fn parse(s: &str) -> Result<Span> {
        let s = s.trim().trim_matches(|c| c == '"' || c == '\'');
        let (sign, body) = match s.strip_prefix('-') {
            Some(r) => (-1, r),
            None => (1, s.strip_prefix('+').unwrap_or(s)),
        };
        let split = body
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(body.len());
        let (num, unit) = body.split_at(split);
        let n: i64 = if num.is_empty() { 1 } else { num.parse()? };
        let unit = Unit::parse(unit.trim()).ok_or_else(|| anyhow!("bad time unit in '{s}'"))?;
        Ok(Span { n: n * sign, unit })
    }

    pub fn approx_secs(&self) -> i64 {
        self.n * self.unit.secs()
    }

    /// Add this span to `ts` using wall-clock arithmetic in `zone`, so
    /// `+1d` across a DST change still lands on the same local time.
    pub fn add_to(&self, ts: i64, zone: Zone) -> i64 {
        match self.unit {
            Unit::Second | Unit::Minute | Unit::Hour => ts + self.approx_secs(),
            Unit::Day | Unit::Week => {
                let days = if self.unit == Unit::Week {
                    self.n * 7
                } else {
                    self.n
                };
                zone.from_local(zone.to_local(ts) + Duration::days(days))
            }
            Unit::Month | Unit::Year => {
                let months = if self.unit == Unit::Year {
                    self.n * 12
                } else {
                    self.n
                };
                let l = zone.to_local(ts);
                zone.from_local(add_months(l.date(), months).and_time(l.time()))
            }
        }
    }
}

pub fn add_months(d: NaiveDate, months: i64) -> NaiveDate {
    let total = d.year() as i64 * 12 + d.month0() as i64 + months;
    let (y, m0) = (total.div_euclid(12) as i32, total.rem_euclid(12) as u32);
    let mut day = d.day();
    loop {
        if let Some(nd) = NaiveDate::from_ymd_opt(y, m0 + 1, day) {
            return nd;
        }
        day -= 1;
    }
}

/// How timestamps are grouped. Sub-day buckets are fixed-width (aligned to
/// the zone's offset when >= 1h); day and longer follow the calendar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Bucket(pub Span);

impl fmt::Display for Bucket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Bucket {
    pub fn secs(n: i64) -> Bucket {
        Bucket(Span {
            n: n.max(1),
            unit: Unit::Second,
        })
    }
    pub fn parse(s: &str) -> Result<Bucket> {
        let sp = Span::parse(s)?;
        if sp.n <= 0 {
            bail!("bucket size must be positive: '{s}'");
        }
        Ok(Bucket(sp).normalized())
    }
    /// Express fixed buckets in the largest exact unit (3600s -> 1h).
    pub fn normalized(self) -> Bucket {
        let Span { n, unit } = self.0;
        if unit >= Unit::Day {
            return self;
        }
        let s = n * unit.secs();
        if s % 86400 == 0 {
            return Bucket(Span {
                n: s / 86400,
                unit: Unit::Day,
            });
        }
        for u in [Unit::Hour, Unit::Minute] {
            if s % u.secs() == 0 {
                return Bucket(Span {
                    n: s / u.secs(),
                    unit: u,
                });
            }
        }
        Bucket(Span {
            n: s,
            unit: Unit::Second,
        })
    }
    pub fn nominal(&self) -> i64 {
        self.0.approx_secs()
    }
    pub fn is_calendar(&self) -> bool {
        self.0.unit >= Unit::Day
    }

    pub fn start_of(&self, ts: i64, zone: Zone) -> i64 {
        let Span { n, unit } = self.0;
        match unit {
            Unit::Second | Unit::Minute | Unit::Hour => {
                let w = n * unit.secs();
                if w >= 3600 {
                    let off = zone.offset_secs(ts);
                    (ts + off).div_euclid(w) * w - off
                } else {
                    ts.div_euclid(w) * w
                }
            }
            Unit::Day => {
                let d = zone.to_local(ts).date();
                let idx = d.num_days_from_ce() as i64;
                let d = d - Duration::days(idx.rem_euclid(n));
                zone.from_local(d.and_time(NaiveTime::MIN))
            }
            Unit::Week => {
                let d = zone.to_local(ts).date();
                let monday = d - Duration::days(d.weekday().num_days_from_monday() as i64);
                let wk = monday.num_days_from_ce() as i64 / 7;
                let monday = monday - Duration::days(7 * wk.rem_euclid(n));
                zone.from_local(monday.and_time(NaiveTime::MIN))
            }
            Unit::Month | Unit::Year => {
                let months = if unit == Unit::Year { n * 12 } else { n };
                let d = zone.to_local(ts).date();
                let idx = d.year() as i64 * 12 + d.month0() as i64;
                let idx = idx - idx.rem_euclid(months);
                let first = NaiveDate::from_ymd_opt(
                    idx.div_euclid(12) as i32,
                    idx.rem_euclid(12) as u32 + 1,
                    1,
                )
                .unwrap();
                zone.from_local(first.and_time(NaiveTime::MIN))
            }
        }
    }

    pub fn next(&self, start: i64, zone: Zone) -> i64 {
        let Span { n, unit } = self.0;
        if unit < Unit::Day {
            return start + n * unit.secs();
        }
        let nx = self.0.add_to(start, zone);
        // Re-anchor in case DST nudged the wall clock.
        let anchored = self.start_of(nx + 3 * 3600, zone);
        if anchored > start {
            anchored
        } else {
            nx
        }
    }

    /// Every bucket start overlapping [from, until).
    pub fn range(&self, from: i64, until: i64, zone: Zone) -> Vec<i64> {
        let mut out = Vec::new();
        let mut t = self.start_of(from, zone);
        while t < until && out.len() < 200_000 {
            out.push(t);
            t = self.next(t, zone);
        }
        out
    }

    /// Smallest "nice" bucket that keeps the range under `max_points`.
    pub fn auto(from: i64, until: i64, max_points: usize) -> Bucket {
        const NICE: &[i64] = &[
            1, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 10800, 21600, 43200,
        ];
        let want = ((until - from).max(1) as f64 / max_points.max(10) as f64).ceil() as i64;
        for &s in NICE {
            if s >= want {
                return Bucket::secs(s).normalized();
            }
        }
        let days = (want as f64 / 86400.0).ceil() as i64;
        if days <= 1 {
            Bucket(Span {
                n: 1,
                unit: Unit::Day,
            })
        } else if days <= 7 {
            Bucket(Span {
                n: 1,
                unit: Unit::Week,
            })
        } else if days <= 31 {
            Bucket(Span {
                n: 1,
                unit: Unit::Month,
            })
        } else {
            Bucket(Span {
                n: 1,
                unit: Unit::Year,
            })
        }
    }

    /// Bucket matching a series' native interval (e.g. daily data -> 1d).
    pub fn from_interval(secs: i64) -> Option<Bucket> {
        if secs <= 0 {
            return None;
        }
        Some(if secs >= 86400 * 27 {
            Bucket(Span {
                n: 1,
                unit: Unit::Month,
            })
        } else if secs >= 86400 * 6 {
            Bucket(Span {
                n: 1,
                unit: Unit::Week,
            })
        } else if secs >= 82800 {
            Bucket(Span {
                n: 1,
                unit: Unit::Day,
            })
        } else {
            Bucket::secs(secs).normalized()
        })
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Parse a `from`/`until` value. Supports:
/// `now`, `-7d`, `-1mon` (Graphite), `now-7d`, `now/d`, `now-1M/M` (Grafana),
/// `today`, `yesterday`, `midnight`, `2026-01-31`, `2026-01-31 12:00`,
/// `12:00_20260131`, `20260131`, epoch seconds or milliseconds.
pub fn parse_time(s: &str, now_ts: i64, zone: Zone, round_up: bool) -> Result<i64> {
    let s = s.trim();
    if s.is_empty() || s == "now" {
        return Ok(now_ts);
    }
    if s.chars().all(|c| c.is_ascii_digit()) && s.len() >= 9 {
        let v: i64 = s.parse()?;
        return Ok(if v > 100_000_000_000 { v / 1000 } else { v });
    }
    let today = zone.from_local(zone.to_local(now_ts).date().and_time(NaiveTime::MIN));
    match s {
        "today" | "midnight" => return Ok(today),
        "yesterday" => {
            return Ok(Span {
                n: -1,
                unit: Unit::Day,
            }
            .add_to(today, zone))
        }
        "tomorrow" => {
            return Ok(Span {
                n: 1,
                unit: Unit::Day,
            }
            .add_to(today, zone))
        }
        _ => {}
    }
    // Relative expressions: optional anchor, then +/- spans, then optional /unit.
    let (anchor, rest) = if let Some(r) = s.strip_prefix("now") {
        (Some(now_ts), r)
    } else if let Some(r) = s.strip_prefix("today") {
        (Some(today), r)
    } else if s.starts_with('-') || s.starts_with('+') {
        (Some(now_ts), s)
    } else {
        (None, s)
    };
    if let Some(mut t) = anchor {
        let (math, round) = match rest.find('/') {
            Some(i) => (&rest[..i], Some(&rest[i + 1..])),
            None => (rest, None),
        };
        let mut i = 0;
        let b = math.as_bytes();
        while i < b.len() {
            let start = i;
            i += 1;
            while i < b.len() && b[i] != b'+' && b[i] != b'-' {
                i += 1;
            }
            t = Span::parse(&math[start..i])?.add_to(t, zone);
        }
        if let Some(u) = round {
            let unit = Unit::parse(u).ok_or_else(|| anyhow!("bad rounding unit '/{u}'"))?;
            let bk = Bucket(Span { n: 1, unit });
            let st = bk.start_of(t, zone);
            t = if round_up { bk.next(st, zone) } else { st };
        }
        return Ok(t);
    }
    parse_absolute(s, zone).ok_or_else(|| anyhow!("cannot parse time '{s}'"))
}

pub fn parse_absolute(s: &str, zone: Zone) -> Option<i64> {
    let s = s.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp());
    }
    if let Some(n) = parse_naive(s) {
        return Some(zone.from_local(n));
    }
    // Graphite: HH:MM_YYYYMMDD
    if let Some((hm, ymd)) = s.split_once('_') {
        let d = NaiveDate::parse_from_str(ymd, "%Y%m%d").ok()?;
        let t = NaiveTime::parse_from_str(hm, "%H:%M").ok()?;
        return Some(zone.from_local(d.and_time(t)));
    }
    None
}

/// Naive (zone-less) date/time in the formats seen in CSVs and SQLite.
pub fn parse_naive(s: &str) -> Option<NaiveDateTime> {
    let s = s.trim();
    for f in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
        "%Y/%m/%d %H:%M:%S",
        "%Y/%m/%d %H:%M",
    ] {
        if let Ok(v) = NaiveDateTime::parse_from_str(s, f) {
            return Some(v);
        }
    }
    for f in ["%Y-%m-%d", "%Y/%m/%d", "%Y%m%d"] {
        if let Ok(d) = NaiveDate::parse_from_str(s, f) {
            return Some(d.and_time(NaiveTime::MIN));
        }
    }
    None
}

/// "HH:MM:SS[.ffff]" -> seconds. Used for duration-valued columns.
pub fn parse_clock_duration(s: &str) -> Option<f64> {
    let mut parts = s.trim().split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let sec: f64 = parts.next().unwrap_or("0").parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(h * 3600.0 + m * 60.0 + sec)
}

pub fn fmt_local(ts: i64, zone: Zone) -> String {
    let n = zone.to_local(ts);
    if n.hour() == 0 && n.minute() == 0 && n.second() == 0 {
        n.format("%Y-%m-%d").to_string()
    } else {
        n.format("%Y-%m-%d %H:%M:%S").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn madrid() -> Zone {
        Zone::parse("Europe/Madrid").unwrap()
    }

    #[test]
    fn spans() {
        assert_eq!(
            Span::parse("7d").unwrap(),
            Span {
                n: 7,
                unit: Unit::Day
            }
        );
        assert_eq!(
            Span::parse("-1mon").unwrap(),
            Span {
                n: -1,
                unit: Unit::Month
            }
        );
        assert_eq!(Span::parse("1M").unwrap().unit, Unit::Month);
        assert_eq!(Span::parse("5m").unwrap().unit, Unit::Minute);
        assert_eq!(Span::parse("1y").unwrap().unit, Unit::Year);
    }

    #[test]
    fn day_buckets_follow_local_midnight_across_dst() {
        let z = madrid();
        let b = Bucket::parse("1d").unwrap();
        // 2026-03-29 is the spring-forward day in Madrid (23h long).
        let from = z.from_local(parse_naive("2026-03-28 12:00").unwrap());
        let until = z.from_local(parse_naive("2026-03-31 00:00").unwrap());
        let r = b.range(from, until, z);
        let names: Vec<_> = r.iter().map(|t| fmt_local(*t, z)).collect();
        assert_eq!(names, vec!["2026-03-28", "2026-03-29", "2026-03-30"]);
        assert_eq!(r[2] - r[1], 23 * 3600);
    }

    #[test]
    fn month_and_week_buckets() {
        let z = madrid();
        let t = z.from_local(parse_naive("2026-10-09 16:00").unwrap());
        assert_eq!(
            fmt_local(Bucket::parse("1mon").unwrap().start_of(t, z), z),
            "2026-10-01"
        );
        assert_eq!(
            fmt_local(Bucket::parse("1w").unwrap().start_of(t, z), z),
            "2026-10-05"
        );
        assert_eq!(
            fmt_local(Bucket::parse("1y").unwrap().start_of(t, z), z),
            "2026-01-01"
        );
    }

    #[test]
    fn time_expressions() {
        let z = madrid();
        let now = z.from_local(parse_naive("2026-10-09 16:30").unwrap());
        let p = |s: &str| fmt_local(parse_time(s, now, z, false).unwrap(), z);
        assert_eq!(p("now/d"), "2026-10-09");
        assert_eq!(p("now-1M/M"), "2026-09-01");
        assert_eq!(p("-1mon"), "2026-09-09 16:30:00");
        assert_eq!(p("today"), "2026-10-09");
        assert_eq!(p("yesterday"), "2026-10-08");
        assert_eq!(p("2026-01-15"), "2026-01-15");
        assert_eq!(p("12:00_20260115"), "2026-01-15 12:00:00");
        assert_eq!(
            fmt_local(parse_time("now/M", now, z, true).unwrap(), z),
            "2026-11-01"
        );
    }

    #[test]
    fn auto_buckets() {
        assert_eq!(Bucket::auto(0, 86400, 800).to_string(), "2min");
        assert_eq!(Bucket::auto(0, 30 * 86400, 800).to_string(), "1h");
        assert_eq!(Bucket::auto(0, 365 * 86400, 800).to_string(), "12h");
        assert_eq!(Bucket::auto(0, 3 * 365 * 86400, 800).to_string(), "1w");
        assert_eq!(Bucket::from_interval(86400).unwrap().to_string(), "1d");
    }
}
