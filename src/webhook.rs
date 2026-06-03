//! Webhook signature verification and parameter handling.

use std::time::SystemTime;

use chrono::{DateTime, Utc};

use crate::errors::TimestampValidationError;

/// Return the current Unix timestamp in seconds.
fn unix_timestamp_now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system clock is before Unix epoch")
        .as_secs()
        .cast_signed()
}

/// Validate webhook timestamp to prevent replay attacks.
///
/// Rejects webhooks that are too old or too far in the future based on `max_age_seconds`. Returns
/// the validated timestamp as a [`DateTime<Utc>`] on success.
pub(crate) fn validate_timestamp(
    timestamp: u32,
    max_offset_seconds: i64,
) -> Result<DateTime<Utc>, TimestampValidationError> {
    let current_timestamp = unix_timestamp_now();
    let timestamp_i64 = i64::from(timestamp);
    let age_seconds = current_timestamp.saturating_sub(timestamp_i64);

    if age_seconds > max_offset_seconds {
        return Err(TimestampValidationError::TooOld {
            age_seconds,
            max_seconds: max_offset_seconds,
        });
    }
    if age_seconds < max_offset_seconds.saturating_neg() {
        return Err(TimestampValidationError::FromFuture {
            offset_seconds: age_seconds.abs(),
            max_seconds: max_offset_seconds,
        });
    }

    Ok(DateTime::from_timestamp(timestamp_i64, 0)
        .expect("timestamp validated to be within representable range"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_AGE: i64 = 300;

    mod validate_timestamp {
        use super::*;

        fn now_as_u32() -> u32 {
            u32::try_from(unix_timestamp_now()).expect("current timestamp fits in usize")
        }

        fn max_age_as_u32() -> u32 {
            u32::try_from(MAX_AGE).expect("MAX_AGE fits in usize")
        }

        #[test]
        fn current_timestamp_is_valid() {
            assert!(validate_timestamp(now_as_u32(), MAX_AGE).is_ok());
        }

        #[test]
        fn very_old_timestamp_is_rejected() {
            assert!(validate_timestamp(1_000_000_000, MAX_AGE).is_err());
        }

        #[test]
        fn far_future_timestamp_is_rejected() {
            assert!(validate_timestamp(4_294_967_295, MAX_AGE).is_err());
        }

        #[test]
        fn exactly_at_boundary_is_valid() {
            let at_boundary = now_as_u32().saturating_sub(max_age_as_u32());
            assert!(validate_timestamp(at_boundary, MAX_AGE).is_ok());
        }

        #[test]
        fn one_second_beyond_boundary_is_rejected() {
            let beyond_boundary = now_as_u32().saturating_sub(max_age_as_u32() + 1);
            assert!(validate_timestamp(beyond_boundary, MAX_AGE).is_err());
        }

        #[test]
        fn near_future_is_valid() {
            let near_future = now_as_u32().saturating_add(10);
            assert!(validate_timestamp(near_future, MAX_AGE).is_ok());
        }

        #[test]
        fn far_future_is_rejected() {
            let far_future = now_as_u32().saturating_add(max_age_as_u32() + 1);
            assert!(validate_timestamp(far_future, MAX_AGE).is_err());
        }
    }
}
