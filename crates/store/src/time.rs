//! Times (upstream issue #146). The bridge stores each time with its UTC
//! offset, in the zone of the computer it runs on (`+01:00`, `+02:00` in
//! summer). Text with different offsets does not sort correctly, so every
//! comparison and sort in SQL goes through SQLite's `julianday()`.

use std::fmt;

use chrono::{
    DateTime, FixedOffset, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Offset,
    TimeZone, Timelike, Utc,
};
use serde::{Serialize, Serializer};

/// A moment with the UTC offset it was stored with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp(pub DateTime<FixedOffset>);

impl Timestamp {
    /// Reads a time as `messages.db` stores it, such as
    /// `2026-11-04 22:00:52+01:00` (Go's database driver writes this form).
    pub fn parse_stored(text: &str) -> Option<Timestamp> {
        match parse_iso(text)? {
            Parsed::WithZone(moment) => Some(Timestamp(moment)),
            Parsed::Naive(_) => None,
        }
    }

    /// For the text output: `2026-11-04 22:00:52 +01:00`.
    pub fn display(&self) -> String {
        let offset = self.0.offset().local_minus_utc().div_euclid(60);
        let sign = if offset >= 0 { '+' } else { '-' };
        let offset = offset.abs();
        format!(
            "{} {sign}{:02}:{:02}",
            self.0.format("%Y-%m-%d %H:%M:%S"),
            offset / 60,
            offset % 60
        )
    }

    /// For JSON, as pydantic writes a datetime: `2026-11-04T22:00:52+01:00`,
    /// with six digits of fraction when there is one, and `Z` for UTC.
    pub fn iso(&self) -> String {
        let mut text = self.0.format("%Y-%m-%dT%H:%M:%S").to_string();
        let micros = self.0.nanosecond() / 1_000;
        if micros != 0 {
            text.push_str(&format!(".{micros:06}"));
        }
        let offset = self.0.offset().local_minus_utc();
        if offset == 0 {
            text.push('Z');
        } else {
            let sign = if offset >= 0 { '+' } else { '-' };
            let offset = offset.abs();
            text.push_str(&format!(
                "{sign}{:02}:{:02}",
                offset / 3600,
                offset / 60 % 60
            ));
            if offset % 60 != 0 {
                text.push_str(&format!(":{:02}", offset % 60));
            }
        }
        text
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.iso())
    }
}

/// A date, or date and time, from the caller (`after`, `before`) is not valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadMoment {
    pub name: &'static str,
    pub value: String,
}

impl fmt::Display for BadMoment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Invalid date format for '{}': {}. Use an ISO-8601 date, or date and time, for example 2026-11-04 or 2026-11-04T21:00:00+01:00.",
            self.name, self.value
        )
    }
}

impl std::error::Error for BadMoment {}

/// An ISO-8601 date, or date and time, as UTC text for `julianday()`
/// (`2026-11-04 20:00:00.000000`). A time on its own is refused: it does
/// not say which day. Without a zone it is local time on this computer; a
/// date alone means midnight. Accepts what Python's `fromisoformat` accepts.
pub fn moment_for_sql(value: &str, name: &'static str) -> Result<String, BadMoment> {
    let bad = || BadMoment {
        name,
        value: value.to_string(),
    };
    let moment = match parse_iso(value.trim()).ok_or_else(bad)? {
        Parsed::WithZone(moment) => moment.with_timezone(&Utc),
        Parsed::Naive(naive) => local_to_utc(naive).ok_or_else(bad)?,
    };
    Ok(moment.format("%Y-%m-%d %H:%M:%S%.6f").to_string())
}

/// Local time to UTC, as Python's `naive.astimezone()` does: an ambiguous
/// time (the hour repeated when clocks go back) is the first one; a time that
/// does not exist (skipped when clocks go forward) uses the offset before
/// the change.
fn local_to_utc(naive: NaiveDateTime) -> Option<DateTime<Utc>> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(moment) => Some(moment.with_timezone(&Utc)),
        LocalResult::Ambiguous(first, _) => Some(first.with_timezone(&Utc)),
        LocalResult::None => {
            let before = naive.checked_sub_signed(chrono::Duration::hours(3))?;
            let offset = match Local.from_local_datetime(&before) {
                LocalResult::Single(m) | LocalResult::Ambiguous(m, _) => m.offset().fix(),
                LocalResult::None => return None,
            };
            offset
                .from_local_datetime(&naive)
                .single()
                .map(|m| m.with_timezone(&Utc))
        }
    }
}

enum Parsed {
    WithZone(DateTime<FixedOffset>),
    Naive(NaiveDateTime),
}

/// Python 3.11's `datetime.fromisoformat`: a date (`2026-11-04`,
/// `20261104`, `2026-W45-3`), then optionally any one character and a time
/// (`21`, `21:00`, `21:00:00`, `210000`, with a fraction after `.` or `,`),
/// then optionally `Z` or an offset (`+01`, `+01:00`, `+0100`,
/// `+01:00:00`).
fn parse_iso(text: &str) -> Option<Parsed> {
    if !text.is_ascii() {
        return None;
    }
    let (date, rest) = parse_date(text)?;
    if rest.is_empty() {
        return Some(Parsed::Naive(date.and_time(NaiveTime::MIN)));
    }
    // Any single character separates the date and the time.
    let rest = &rest[1..];
    let zone_at = rest.find(['Z', 'z', '+', '-']).unwrap_or(rest.len());
    let (time_text, zone_text) = rest.split_at(zone_at);
    let time = parse_time(time_text)?;
    let naive = date.and_time(time);
    if zone_text.is_empty() {
        return Some(Parsed::Naive(naive));
    }
    let offset = parse_zone(zone_text)?;
    offset
        .from_local_datetime(&naive)
        .single()
        .map(Parsed::WithZone)
}

fn parse_date(text: &str) -> Option<(NaiveDate, &str)> {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let num = |s: &str| -> Option<u32> { digits(s).then(|| s.parse().ok()).flatten() };
    let bytes = text.as_bytes();
    // Week date: 2026-W45-3, 2026W453, 2026-W45, 2026W45.
    if text.len() >= 7 && (bytes.get(4) == Some(&b'W') || bytes.get(5) == Some(&b'W')) {
        let year = num(&text[..4])? as i32;
        let (week, day, used) = if bytes[4] == b'-' {
            let week = num(text.get(6..8)?)?;
            if bytes.get(8) == Some(&b'-') {
                (week, num(text.get(9..10)?)?, 10)
            } else {
                (week, 1, 8)
            }
        } else {
            let week = num(text.get(5..7)?)?;
            match text.get(7..8).and_then(num) {
                Some(day) => (week, day, 8),
                None => (week, 1, 7),
            }
        };
        let weekday = chrono::Weekday::try_from(u8::try_from(day.checked_sub(1)?).ok()?).ok()?;
        let date = NaiveDate::from_isoywd_opt(year, week, weekday)?;
        return Some((date, &text[used..]));
    }
    // 2026-11-04 or 20261104.
    let (year, month, day, used) = if bytes.get(4) == Some(&b'-') {
        if bytes.get(7) != Some(&b'-') {
            return None;
        }
        (
            num(text.get(..4)?)?,
            num(text.get(5..7)?)?,
            num(text.get(8..10)?)?,
            10,
        )
    } else {
        (
            num(text.get(..4)?)?,
            num(text.get(4..6)?)?,
            num(text.get(6..8)?)?,
            8,
        )
    };
    let date = NaiveDate::from_ymd_opt(year as i32, month, day)?;
    Some((date, &text[used..]))
}

fn parse_time(text: &str) -> Option<NaiveTime> {
    let (main, fraction) = match text.find(['.', ',']) {
        Some(at) => (&text[..at], Some(&text[at + 1..])),
        None => (text, None),
    };
    let parts: Vec<&str> = if main.contains(':') {
        main.split(':').collect()
    } else {
        main.as_bytes()
            .chunks(2)
            .map(|c| std::str::from_utf8(c).unwrap_or(""))
            .collect()
    };
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|p| p.len() != 2) {
        return None;
    }
    let field = |i: usize| -> Option<u32> {
        match parts.get(i) {
            Some(p) if p.bytes().all(|b| b.is_ascii_digit()) => p.parse().ok(),
            Some(_) => None,
            None => Some(0),
        }
    };
    let (hour, minute, second) = (field(0)?, field(1)?, field(2)?);
    let nanos = match fraction {
        None => 0,
        Some(f) => {
            // Only after seconds, 1 to 9 digits; Python keeps microseconds.
            if parts.len() != 3
                || f.is_empty()
                || f.len() > 9
                || !f.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            let micros: u32 = format!("{f:0<6}")[..6].parse().ok()?;
            micros * 1_000
        }
    };
    NaiveTime::from_hms_nano_opt(hour, minute, second, nanos)
}

fn parse_zone(text: &str) -> Option<FixedOffset> {
    if text.eq_ignore_ascii_case("z") {
        return FixedOffset::east_opt(0);
    }
    let sign = match text.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let time = parse_time(&text[1..])?;
    if time.nanosecond() != 0 {
        return None;
    }
    let seconds = (time.hour() * 3600 + time.minute() * 60 + time.second()) as i32;
    if seconds >= 24 * 3600 {
        return None;
    }
    FixedOffset::east_opt(sign * seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_times() {
        let t = Timestamp::parse_stored("2026-11-04 22:00:52+01:00").unwrap();
        assert_eq!(t.display(), "2026-11-04 22:00:52 +01:00");
        assert_eq!(t.iso(), "2026-11-04T22:00:52+01:00");
        let utc = Timestamp::parse_stored("2026-11-04 20:45:00+00:00").unwrap();
        assert_eq!(utc.display(), "2026-11-04 20:45:00 +00:00");
        assert_eq!(utc.iso(), "2026-11-04T20:45:00Z");
        let fraction = Timestamp::parse_stored("2026-01-02 03:04:05.123-05:00").unwrap();
        assert_eq!(fraction.iso(), "2026-01-02T03:04:05.123000-05:00");
        assert_eq!(fraction.display(), "2026-01-02 03:04:05 -05:00");
        assert!(Timestamp::parse_stored("2026-11-04 22:00:52").is_none());
    }

    #[test]
    fn moments_with_zone() {
        for (input, want) in [
            ("2026-11-04T21:00:00+01:00", "2026-11-04 20:00:00.000000"),
            ("2026-11-04T20:00:00Z", "2026-11-04 20:00:00.000000"),
            ("2026-11-04 20:00Z", "2026-11-04 20:00:00.000000"),
            ("20261104T210000+0100", "2026-11-04 20:00:00.000000"),
            ("2026-11-04T21+01", "2026-11-04 20:00:00.000000"),
            ("2026-11-04T20:00:00.5Z", "2026-11-04 20:00:00.500000"),
            ("2026-W45-3T20:00Z", "2026-11-04 20:00:00.000000"),
        ] {
            assert_eq!(moment_for_sql(input, "after").unwrap(), want, "{input}");
        }
    }

    #[test]
    fn refused_moments() {
        for input in [
            "21:00",
            "yesterday",
            "2026-13-01",
            "2026-11-04T25:00",
            "",
            "2026-11-4",
        ] {
            let err = moment_for_sql(input, "after").unwrap_err();
            assert!(
                err.to_string()
                    .starts_with("Invalid date format for 'after'"),
                "{input}"
            );
        }
    }
}
