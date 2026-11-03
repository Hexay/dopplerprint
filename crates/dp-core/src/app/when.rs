//! `--start` / `--start-tolerance` parsing into a recording-start prior (UNIX seconds UTC).

use super::NeedsInput;
use crate::adsb::manifest::{civil_date, days_from_civil};

/// Default ± half-width of the start prior; rationale in docs/USAGE.md#time.
pub const DEFAULT_TOLERANCE_S: f64 = 120.0;
const EXAMPLE: &str = "--start \"2026-05-14 10:02 -07:00\"";

/// Bounds on the UNIX time of the first audio sample.
#[derive(Clone, Debug, serde::Serialize)]
pub struct StartPrior {
    pub lo: f64,
    pub hi: f64,
    /// The time that was given (by the user or the file).
    pub nominal: f64,
    /// Where `nominal` came from, for the report.
    pub source: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParsedStart {
    pub t: f64,
    pub tolerance_s: Option<f64>,
}

#[derive(Debug, PartialEq)]
pub enum InstantError {
    NoZone,
    Unreadable,
}

pub fn missing_start() -> NeedsInput {
    NeedsInput(format!(
        "Need the time the recording started: add {EXAMPLE} (your local time and its UTC offset; append \"±10m\" if unsure)."
    ))
}

/// `"2026-05-14 10:02 -07:00"`, `"2026-05-14T17:02:00Z"`, optionally followed by `±10m` / `+-10m` / `+/-10m`.
pub fn parse_start(s: &str) -> Result<ParsedStart, NeedsInput> {
    let (when, tol) = split_tolerance(s);
    let tolerance_s = match tol {
        Some(d) => Some(parse_duration(d).ok_or_else(|| bad_tolerance(d))?),
        None => None,
    };
    match parse_instant(when) {
        Ok(t) => Ok(ParsedStart { t, tolerance_s }),
        Err(InstantError::NoZone) => Err(NeedsInput(format!(
            "--start \"{}\" has no UTC offset; add one, e.g. {EXAMPLE} (or Z for UTC).",
            when.trim()
        ))),
        Err(InstantError::Unreadable) => Err(NeedsInput(format!(
            "Can't read --start \"{}\"; use e.g. {EXAMPLE} or \"2026-05-14T17:02:00Z\".",
            when.trim()
        ))),
    }
}

pub fn parse_tolerance(s: &str) -> Result<f64, NeedsInput> {
    parse_duration(s).ok_or_else(|| bad_tolerance(s))
}

fn bad_tolerance(s: &str) -> NeedsInput {
    NeedsInput(format!("Can't read the start tolerance \"{}\"; use e.g. 90s, 10m or 1h.", s.trim()))
}

fn split_tolerance(s: &str) -> (&str, Option<&str>) {
    for sep in ["±", "+/-", "+-"] {
        if let Some((a, b)) = s.split_once(sep) {
            return (a, Some(b));
        }
    }
    (s, None)
}

/// `90`, `90s`, `10m`, `10 min`, `1.5h` → seconds.
pub fn parse_duration(s: &str) -> Option<f64> {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let v: f64 = num.parse().ok()?;
    let scale = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => 1.0,
        "m" | "min" | "mins" | "minute" | "minutes" => 60.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3600.0,
        _ => return None,
    };
    (v.is_finite() && v >= 0.0).then_some(v * scale)
}

/// A date-time with an explicit UTC offset (or `Z`/`UTC`), or bare UNIX seconds.
pub fn parse_instant(s: &str) -> Result<f64, InstantError> {
    let s = s.trim();
    if let Ok(v) = s.parse::<f64>() {
        return if v > 1e8 { Ok(v) } else { Err(InstantError::Unreadable) };
    }
    let date_len = s.find(['T', 't', ' ']).ok_or(InstantError::Unreadable)?;
    let (date, rest) = (&s[..date_len], s[date_len + 1..].trim_start());
    let time_len = rest.find(|c: char| !(c.is_ascii_digit() || c == ':' || c == '.')).unwrap_or(rest.len());
    let (time, zone) = rest.split_at(time_len);
    let days = parse_date(date).ok_or(InstantError::Unreadable)?;
    let secs = parse_time(time).ok_or(InstantError::Unreadable)?;
    let offset = parse_zone(zone)?;
    Ok(days as f64 * 86_400.0 + secs - offset)
}

fn parse_date(s: &str) -> Option<i64> {
    let p: Vec<i64> = s.split(['-', '/']).map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let [y, m, d] = p.as_slice() else { return None };
    ((1970..=2100).contains(y) && (1..=12).contains(m) && (1..=31).contains(d)).then(|| days_from_civil(*y, *m, *d))
}

fn parse_time(s: &str) -> Option<f64> {
    let p: Vec<f64> = s.split(':').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let (h, m, sec) = match p.as_slice() {
        [h, m] => (*h, *m, 0.0),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };
    (h < 24.0 && m < 60.0 && sec < 61.0 && h.fract() == 0.0 && m.fract() == 0.0).then_some(h * 3600.0 + m * 60.0 + sec)
}

/// Offset east of UTC in seconds. Accepts `Z`, `UTC`, `GMT`, `±HH:MM`, `±HHMM`, `±H`, and `UTC±H[:MM]`.
fn parse_zone(z: &str) -> Result<f64, InstantError> {
    let z = z.trim();
    if z.is_empty() {
        return Err(InstantError::NoZone);
    }
    let upper = z.to_ascii_uppercase();
    let rest = upper.strip_prefix("UTC").or_else(|| upper.strip_prefix("GMT")).unwrap_or(&upper).trim();
    if rest.is_empty() || rest == "Z" {
        return Ok(0.0);
    }
    let (sign, digits) = match rest.chars().next() {
        Some('+') => (1.0, &rest[1..]),
        Some('-') => (-1.0, &rest[1..]),
        Some('−') => (-1.0, &rest['−'.len_utf8()..]),
        _ => return Err(InstantError::Unreadable),
    };
    let (h, m) = match digits.split_once(':') {
        Some((h, m)) => (h, m),
        None if digits.len() == 4 => digits.split_at(2),
        None => (digits, "0"),
    };
    let (h, m): (f64, f64) = (h.parse().map_err(|_| InstantError::Unreadable)?, m.parse().map_err(|_| InstantError::Unreadable)?);
    if h > 14.0 || m >= 60.0 {
        return Err(InstantError::Unreadable);
    }
    Ok(sign * (h * 3600.0 + m * 60.0))
}

/// `2026-05-14 17:02:13 UTC`.
pub fn fmt_utc(t: f64) -> String {
    let s = t.round() as i64;
    let sod = s.rem_euclid(86_400);
    format!("{} {:02}:{:02}:{:02} UTC", civil_date(s as f64), sod / 3600, sod % 3600 / 60, sod % 60)
}

/// `13 s`, `4 min 05 s`, `1 h 02 min`.
pub fn fmt_duration(s: f64) -> String {
    let s = s.abs().round() as i64;
    match s {
        0..60 => format!("{s} s"),
        60..3600 => format!("{} min {:02} s", s / 60, s % 60),
        _ => format!("{} h {:02} min", s / 3600, s % 3600 / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: f64 = 1_778_778_120.0; // 2026-05-14T17:02:00Z

    #[test]
    fn instants_with_offsets() {
        for s in ["2026-05-14T17:02:00Z", "2026-05-14 17:02 Z", "2026-05-14 17:02 UTC", "2026-05-14T10:02:00-07:00",
            "2026-05-14 10:02 -07:00", "2026-05-14 10:02 -0700", "2026-05-14 10:02-7", "2026-05-14 10:02 UTC-7",
            "2026-05-14 22:32 +05:30", "2026/05/14 17:02:00 GMT", "1778778120"]
        {
            assert_eq!(parse_instant(s), Ok(T), "{s}");
        }
        assert_eq!(parse_instant("2026-05-14T17:02:30.5Z"), Ok(T + 30.5));
    }

    #[test]
    fn instants_rejected() {
        assert_eq!(parse_instant("2026-05-14 10:02"), Err(InstantError::NoZone));
        for s in ["yesterday", "2026-13-01 10:00Z", "2026-05-14 25:00Z", "2026-05-14 10:02 PST", "12"] {
            assert_eq!(parse_instant(s), Err(InstantError::Unreadable), "{s}");
        }
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("90"), Some(90.0));
        assert_eq!(parse_duration("10m"), Some(600.0));
        assert_eq!(parse_duration(" 10 min"), Some(600.0));
        assert_eq!(parse_duration("1.5h"), Some(5400.0));
        assert_eq!(parse_duration("30 s"), Some(30.0));
        assert_eq!(parse_duration("ten"), None);
        assert_eq!(parse_duration("5 days"), None);
    }

    #[test]
    fn start_with_tolerance() {
        let p = parse_start("2026-05-14 10:02 -07:00 ±10m").unwrap();
        assert_eq!(p, ParsedStart { t: T, tolerance_s: Some(600.0) });
        assert_eq!(parse_start("2026-05-14T17:02Z +-90s").unwrap().tolerance_s, Some(90.0));
        assert_eq!(parse_start("2026-05-14T17:02Z +/- 2 min").unwrap().tolerance_s, Some(120.0));
        assert_eq!(parse_start("2026-05-14T17:02Z").unwrap().tolerance_s, None);
    }

    #[test]
    fn needs_input_messages_say_what_to_add() {
        let no_zone = parse_start("2026-05-14 10:02").unwrap_err().0;
        assert!(no_zone.contains("no UTC offset") && no_zone.contains("-07:00"), "{no_zone}");
        let bad = parse_start("last tuesday").unwrap_err().0;
        assert!(bad.starts_with("Can't read --start") && bad.contains("2026-05-14"), "{bad}");
        let tol = parse_start("2026-05-14T17:02Z ±soon").unwrap_err().0;
        assert!(tol.contains("tolerance") && tol.contains("10m"), "{tol}");
        let missing = missing_start().0;
        assert!(missing.contains("--start") && missing.ends_with('.') && !missing.contains('\n'), "{missing}");
    }

    #[test]
    fn formatting() {
        assert_eq!(fmt_utc(T + 13.0), "2026-05-14 17:02:13 UTC");
        assert_eq!(fmt_duration(13.2), "13 s");
        assert_eq!(fmt_duration(-245.0), "4 min 05 s");
        assert_eq!(fmt_duration(3720.0), "1 h 02 min");
    }
}
