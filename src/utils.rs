//! Common utilities shared across modules.

use chrono::Utc;

/// Get current UTC timestamp in seconds since UNIX_EPOCH.
///
/// Uses chrono for accurate cross-platform timestamp.
pub fn get_utc_timestamp() -> u64 {
    Utc::now().timestamp() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_utc_timestamp() {
        let ts = get_utc_timestamp();
        // Should be a reasonable Unix timestamp (after 2020)
        assert!(ts > 1577836800, "Timestamp should be after 2020-01-01");
    }
}
