//! RFC 3339 UTC timestamps (whole seconds) without a time crate: civil-from-days (Hinnant).
use std::time::{SystemTime, UNIX_EPOCH};

/// `YYYY-MM-DDTHH:MM:SSZ`; `None` for times before 1970.
pub(crate) fn rfc3339(t: SystemTime) -> Option<String> {
    let secs = i64::try_from(t.duration_since(UNIX_EPOCH).ok()?.as_secs()).ok()?;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    Some(format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    ))
}

/// The current time; the epoch if the clock is before 1970.
pub(crate) fn now_rfc3339() -> String {
    rfc3339(SystemTime::now()).unwrap_or_else(|| "1970-01-01T00:00:00Z".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_instants() {
        let at = |s| rfc3339(UNIX_EPOCH + std::time::Duration::from_secs(s)).unwrap();
        assert_eq!(at(0), "1970-01-01T00:00:00Z");
        assert_eq!(at(1_782_000_000), "2026-06-21T00:00:00Z");
        assert_eq!(at(951_782_400 + 86_399), "2000-02-29T23:59:59Z");
    }
}
