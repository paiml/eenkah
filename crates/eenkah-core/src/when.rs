//! Human-friendly dates for sentences: "today", "1 day ago", "3 weeks ago", "tomorrow". The web
//! app uses the same words (`ui/src/time.ts`). Machine-readable output (JSON) keeps ISO dates.

use chrono::NaiveDate;

fn plural(n: i64, unit: &str) -> String {
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

/// `n` whole days before today as words (negative = in the future).
pub fn days(n: i64) -> String {
    match n {
        0 => "today".into(),
        -1 => "tomorrow".into(),
        n if n < 0 => format!("in {}", plural(-n, "day")),
        1..=6 => format!("{} ago", plural(n, "day")),
        7..=29 => format!("{} ago", plural(n / 7, "week")),
        30..=364 => format!("{} ago", plural(((n as f64 / 30.44).floor() as i64).max(1), "month")),
        _ => format!("{} ago", plural((n as f64 / 365.25).floor() as i64, "year")),
    }
}

/// `day` relative to `today`.
pub fn day(day: NaiveDate, today: NaiveDate) -> String {
    days((today - day).num_days())
}

/// The morning of `day`, relative to `today`: "this morning", "tomorrow morning", "3 days ago".
pub fn morning(day: NaiveDate, today: NaiveDate) -> String {
    match (today - day).num_days() {
        0 => "this morning".into(),
        -1 => "tomorrow morning".into(),
        n => days(n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_for_each_range() {
        let cases = [
            (0, "today"),
            (1, "1 day ago"),
            (6, "6 days ago"),
            (7, "1 week ago"),
            (20, "2 weeks ago"),
            (31, "1 month ago"),
            (200, "6 months ago"),
            (400, "1 year ago"),
            (-1, "tomorrow"),
            (-3, "in 3 days"),
        ];
        for (n, w) in cases {
            assert_eq!(days(n), w, "{n}");
        }
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(morning(d("2026-10-14"), d("2026-10-13")), "tomorrow morning");
    }
}
