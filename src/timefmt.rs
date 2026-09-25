//! Calendar maths without a time crate. Unix seconds unless a name says `_ms`.

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    /// 0 = Sunday … 6 = Saturday
    pub weekday: u32,
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = (if m <= 2 { y - 1 } else { y }) as i64;
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let d = d as i64;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of `days_from_civil`.
pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

/// Breaks a Unix time (seconds) into UTC calendar parts. The platform layer
/// provides the local-time equivalent; tests use this one.
pub fn utc_parts(unix_s: i64) -> LocalTime {
    let days = unix_s.div_euclid(86_400);
    let secs = unix_s.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    LocalTime {
        year,
        month,
        day,
        hour: (secs / 3_600) as u32,
        minute: ((secs % 3_600) / 60) as u32,
        weekday: (days + 4).rem_euclid(7) as u32, // 1970-01-01 was a Thursday
    }
}

/// Parses `YYYY-MM-DD[T ]HH:MM:SS[.frac][Z|±HH:MM|±HHMM]`. No offset means UTC.
/// Returns Unix seconds.
pub fn parse_iso8601(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    if b[4] != b'-'
        || b[7] != b'-'
        || (b[10] != b'T' && b[10] != b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> {
        let part = s.get(from..to)?;
        if part.bytes().all(|c| c.is_ascii_digit()) {
            part.parse::<i64>().ok()
        } else {
            None
        }
    };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, se) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        while b.get(i).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
        }
    }
    let offset = match b.get(i) {
        None => 0,
        Some(b'Z') | Some(b'z') => {
            if i + 1 != b.len() {
                return None;
            }
            0
        }
        Some(&sign) if sign == b'+' || sign == b'-' => {
            let oh = num(i + 1, i + 3)?;
            let om = if b.get(i + 3) == Some(&b':') {
                num(i + 4, i + 6)?
            } else {
                num(i + 3, i + 5).unwrap_or(0)
            };
            let o = oh * 3_600 + om * 60;
            if sign == b'+' {
                o
            } else {
                -o
            }
        }
        _ => return None,
    };
    Some(
        days_from_civil(y as i32, mo as u32, d as u32) * 86_400 + h * 3_600 + mi * 60 + se - offset,
    )
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        for z in [-800_000i64, -1, 0, 1, 11_017, 20_000, 400_000] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z, "day {z}");
        }
    }

    #[test]
    fn parses_iso_variants() {
        assert_eq!(parse_iso8601("2001-09-09T01:46:40Z"), Some(1_000_000_000));
        assert_eq!(
            parse_iso8601("2001-09-09T01:46:40.123456Z"),
            Some(1_000_000_000)
        );
        assert_eq!(
            parse_iso8601("2001-09-09T01:46:40+00:00"),
            Some(1_000_000_000)
        );
        assert_eq!(
            parse_iso8601("2001-09-09T08:46:40+07:00"),
            Some(1_000_000_000)
        );
        assert_eq!(
            parse_iso8601("2001-09-08T20:46:40-05:00"),
            Some(1_000_000_000)
        );
        assert_eq!(parse_iso8601("2001-09-09 01:46:40"), Some(1_000_000_000));
        assert_eq!(parse_iso8601("2001-09-09T01:46:40"), Some(1_000_000_000));
    }

    #[test]
    fn rejects_garbage() {
        for s in [
            "",
            "yesterday",
            "2001-13-01T00:00:00Z",
            "2001-09-09",
            "2001-09-09T25:00:00Z",
            "2001-09-09T01:46:40X",
        ] {
            assert_eq!(parse_iso8601(s), None, "{s}");
        }
    }

    #[test]
    fn utc_parts_of_known_instant() {
        let t = utc_parts(1_000_000_000);
        assert_eq!(
            (t.year, t.month, t.day, t.hour, t.minute, t.weekday),
            (2001, 9, 9, 1, 46, 0)
        );
        // 2026-09-28 is a Monday
        let mon = parse_iso8601("2026-09-28T09:00:00Z").unwrap();
        assert_eq!(utc_parts(mon).weekday, 1);
    }

    #[test]
    fn now_is_plausible() {
        assert!(now_ms() > 1_700_000_000_000);
    }
}
