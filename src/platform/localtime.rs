use crate::timefmt::{utc_parts, LocalTime};
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

/// Unix seconds → local calendar parts, honouring the DST rules in force on that date.
pub fn local_parts(unix_s: i64) -> LocalTime {
    let ticks = (unix_s + 11_644_473_600).max(0) as u64 * 10_000_000;
    let ft = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut loc = SYSTEMTIME::default();
    unsafe {
        if FileTimeToSystemTime(&ft, &mut utc).is_err()
            || SystemTimeToTzSpecificLocalTime(None, &utc, &mut loc).is_err()
        {
            return utc_parts(unix_s);
        }
    }
    LocalTime {
        year: loc.wYear as i32,
        month: loc.wMonth as u32,
        day: loc.wDay as u32,
        hour: loc.wHour as u32,
        minute: loc.wMinute as u32,
        weekday: loc.wDayOfWeek as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_is_within_a_day_of_utc() {
        let now = crate::timefmt::now_ms() / 1000;
        let l = local_parts(now);
        let u = utc_parts(now);
        let to_min = |t: &LocalTime| {
            crate::timefmt::days_from_civil(t.year, t.month, t.day) * 1440
                + (t.hour * 60 + t.minute) as i64
        };
        assert!(
            (to_min(&l) - to_min(&u)).abs() <= 14 * 60,
            "offset within ±14 h"
        );
        assert_eq!(
            l.minute % 15,
            u.minute % 15,
            "offsets are whole quarter hours"
        );
    }
}
