//! Time helpers. All timestamps in the database are Unix epoch milliseconds
//! (i64); manifests render them as RFC 3339 strings.

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Current time as Unix epoch milliseconds.
pub fn now_ms() -> i64 {
    (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

/// Parse an RFC 3339 timestamp back to Unix epoch milliseconds.
pub fn rfc3339_to_ms(s: &str) -> Option<i64> {
    OffsetDateTime::parse(s, &Rfc3339)
        .ok()
        .map(|dt| (dt.unix_timestamp_nanos() / 1_000_000) as i64)
}

/// Render epoch milliseconds as an RFC 3339 / ISO 8601 UTC string.
pub fn ms_to_rfc3339(ms: i64) -> String {
    let nanos = (ms as i128) * 1_000_000;
    match OffsetDateTime::from_unix_timestamp_nanos(nanos) {
        Ok(dt) => dt.format(&Rfc3339).unwrap_or_else(|_| ms.to_string()),
        Err(_) => ms.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_is_monotonic_enough() {
        let a = now_ms();
        let b = now_ms();
        assert!(b >= a);
        assert!(a > 1_700_000_000_000); // sanity: after 2023.
    }

    #[test]
    fn rfc3339_formats_a_known_instant() {
        // 2021-01-01T00:00:00Z == 1609459200000 ms
        assert_eq!(ms_to_rfc3339(1_609_459_200_000), "2021-01-01T00:00:00Z");
    }
}
