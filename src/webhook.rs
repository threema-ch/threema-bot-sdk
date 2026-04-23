//! Webhook signature verification and parameter handling.

use std::time::SystemTime;

use crate::errors::WebhookError;

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
/// Rejects webhooks that are too old or too far in the future based on
/// `max_age_seconds`. Returns the parsed timestamp on success.
pub(crate) fn validate_timestamp(
    timestamp: usize,
    max_offset_seconds: i64,
) -> Result<usize, WebhookError> {
    let current_timestamp = unix_timestamp_now();
    #[expect(clippy::cast_possible_wrap, reason = "timestamp fits in i64")]
    let timestamp_i64 = timestamp as i64;
    let age_seconds = current_timestamp.saturating_sub(timestamp_i64);

    if age_seconds > max_offset_seconds {
        return Err(WebhookError::TooOld {
            age_seconds,
            max_seconds: max_offset_seconds,
        });
    }
    if age_seconds < max_offset_seconds.saturating_neg() {
        return Err(WebhookError::FromFuture {
            offset_seconds: age_seconds.abs(),
            max_seconds: max_offset_seconds,
        });
    }

    Ok(timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_AGE: i64 = 300;

    mod validate_timestamp {
        use super::*;

        fn now_as_usize() -> usize {
            usize::try_from(unix_timestamp_now()).expect("current timestamp fits in usize")
        }

        fn max_age_as_usize() -> usize {
            usize::try_from(MAX_AGE).expect("MAX_AGE fits in usize")
        }

        #[test]
        fn current_timestamp_is_valid() {
            assert!(validate_timestamp(now_as_usize(), MAX_AGE).is_ok());
        }

        #[test]
        fn very_old_timestamp_is_rejected() {
            assert!(validate_timestamp(1_000_000_000, MAX_AGE).is_err());
        }

        #[test]
        fn far_future_timestamp_is_rejected() {
            assert!(validate_timestamp(9_999_999_999_999, MAX_AGE).is_err());
        }

        #[test]
        fn exactly_at_boundary_is_valid() {
            let at_boundary = now_as_usize().saturating_sub(max_age_as_usize());
            assert!(validate_timestamp(at_boundary, MAX_AGE).is_ok());
        }

        #[test]
        fn one_second_beyond_boundary_is_rejected() {
            let beyond_boundary = now_as_usize().saturating_sub(max_age_as_usize() + 1);
            assert!(validate_timestamp(beyond_boundary, MAX_AGE).is_err());
        }

        #[test]
        fn near_future_is_valid() {
            let near_future = now_as_usize().saturating_add(10);
            assert!(validate_timestamp(near_future, MAX_AGE).is_ok());
        }

        #[test]
        fn far_future_is_rejected() {
            let far_future = now_as_usize().saturating_add(max_age_as_usize() + 1);
            assert!(validate_timestamp(far_future, MAX_AGE).is_err());
        }
    }
}
