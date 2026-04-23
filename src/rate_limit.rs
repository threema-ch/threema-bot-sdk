//! Per-user rate limiting with automatic cleanup.
//!
//! Provides rate limiting using fixed windows with per-minute and per-hour limits.

use std::time::{Duration, Instant};

use moka::sync::Cache as SyncCache;
use threema_gateway::protocol::ThreemaId;

/// Rate limiting configuration.
#[derive(Debug, Clone)]
pub(crate) struct RateLimitConfig {
    /// Maximum messages per minute per user.
    pub(crate) messages_per_minute: u32,
    /// Maximum messages per hour per user.
    pub(crate) messages_per_hour: u32,
}

/// Result of a rate limit check.
#[derive(Debug, Clone)]
pub(crate) enum RateLimitResult {
    /// Request is allowed.
    Allowed,
    /// Request is rate limited.
    Limited {
        /// Human-readable message about the rate limit.
        message: String,
    },
}

/// A fixed-window counter for a single user and time window.
#[derive(Debug, Clone)]
struct Window {
    count: u32,
    started_at: Instant,
    duration: Duration,
}

impl Window {
    fn new(duration: Duration) -> Self {
        Self {
            count: 0,
            started_at: Instant::now(),
            duration,
        }
    }

    /// Increment the counter if within the current window.
    ///
    /// Returns `true` if the count is within the given limit, `false` if exceeded.
    ///
    /// Note: This uses a fixed window, so up to `2 * limit` messages may be sent in a short burst
    /// across a window boundary (e.g. `limit` messages just before expiry and `limit` more just
    /// after reset).
    fn check_and_increment(&mut self, limit: u32) -> bool {
        if self.started_at.elapsed() >= self.duration {
            // Window expired: reset
            self.count = 1;
            self.started_at = Instant::now();
            return true;
        }
        self.count = self.count.saturating_add(1);
        self.count <= limit
    }

    /// Time remaining in the current window.
    fn time_remaining(&self) -> Duration {
        self.duration.saturating_sub(self.started_at.elapsed())
    }
}

/// Manager for simple per-user rate limiters.
///
/// ## How it works
///
/// Each user is tracked independently with two fixed-window counters: One for the per-minute limit
/// and one for the per-hour limit. The window starts when the user sends their first message and
/// resets once the window duration (60s or 3600s) has elapsed, regardless of activity in between.
///
/// Note: Because this uses fixed windows rather than sliding windows, a user can theoretically send
/// up to twice the configured limit in a short burst straddling a window boundary. For human-paced
/// chat this is not a practical concern.
///
/// Inactive users are automatically cleaned up via TTL caches to prevent unbounded memory growth.
pub(crate) struct RateLimiterManager {
    config: RateLimitConfig,
    minute_windows: SyncCache<ThreemaId, Window>,
    hour_windows: SyncCache<ThreemaId, Window>,
}

impl RateLimiterManager {
    /// Create a new rate limiter manager with the default user cache capacity of `10_000`
    pub(crate) fn new(config: RateLimitConfig) -> Self {
        Self::with_capacity(config, 10_000)
    }

    /// Create a new rate limiter manager with the specified user cache capacity
    pub(crate) fn with_capacity(config: RateLimitConfig, capacity: u64) -> Self {
        let minute_windows = SyncCache::builder()
            .time_to_idle(Duration::from_secs(60 + 10)) // Expire after 1 minute + 10 seconds
            .max_capacity(capacity)
            .build();

        let hour_windows = SyncCache::builder()
            .time_to_idle(Duration::from_mins(61)) // Expire after 1 hour + 60 seconds grace
            .max_capacity(capacity)
            .build();

        Self {
            config,
            minute_windows,
            hour_windows,
        }
    }

    /// Check if a request from a user is within rate limits.
    ///
    /// Returns [`RateLimitResult::Allowed`] if the request should proceed,
    /// or [`RateLimitResult::Limited`] with a message if rate limited.
    pub(crate) fn check(&self, user_id: ThreemaId) -> RateLimitResult {
        let key = user_id;

        // Check and update per-minute window
        let mut minute_window = self
            .minute_windows
            .get(&key)
            .unwrap_or_else(|| Window::new(Duration::from_mins(1)));

        if !minute_window.check_and_increment(self.config.messages_per_minute) {
            let wait_time = minute_window.time_remaining();
            let wait_text = format_duration(wait_time);
            self.minute_windows.insert(key, minute_window);
            return RateLimitResult::Limited {
                message: format!("Rate limit exceeded. Try again in {wait_text}."),
            };
        }
        self.minute_windows.insert(key, minute_window);

        // Check and update per-hour window
        let mut hour_window = self
            .hour_windows
            .get(&key)
            .unwrap_or_else(|| Window::new(Duration::from_hours(1)));

        if !hour_window.check_and_increment(self.config.messages_per_hour) {
            let wait_time = hour_window.time_remaining();
            let wait_text = format_duration(wait_time);
            self.hour_windows.insert(key, hour_window);
            return RateLimitResult::Limited {
                message: format!("Rate limit exceeded. Try again in {wait_text}."),
            };
        }
        self.hour_windows.insert(key, hour_window);

        RateLimitResult::Allowed
    }
}

/// Format a duration in human-readable format.
///
/// Examples: "5 seconds", "2 minutes", "1 minute and 30 seconds"
#[expect(
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    reason = "intentional integer arithmetic for human-readable time formatting"
)]
fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();

    if total_seconds < 60 {
        format!(
            "{} second{}",
            total_seconds,
            if total_seconds == 1 { "" } else { "s" }
        )
    } else if total_seconds < 3600 {
        let minutes = total_seconds / 60;
        let remaining_seconds = total_seconds % 60;
        if remaining_seconds == 0 {
            format!("{} minute{}", minutes, if minutes == 1 { "" } else { "s" })
        } else {
            format!(
                "{} minute{} and {} second{}",
                minutes,
                if minutes == 1 { "" } else { "s" },
                remaining_seconds,
                if remaining_seconds == 1 { "" } else { "s" }
            )
        }
    } else {
        let hours = total_seconds / 3600;
        format!("{} hour{}", hours, if hours == 1 { "" } else { "s" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod format_duration {
        use super::*;

        #[test]
        fn seconds() {
            assert_eq!(format_duration(Duration::from_secs(1)), "1 second");
            assert_eq!(format_duration(Duration::from_secs(5)), "5 seconds");
            assert_eq!(format_duration(Duration::from_secs(59)), "59 seconds");
        }

        #[test]
        fn minutes() {
            assert_eq!(format_duration(Duration::from_mins(1)), "1 minute");
            assert_eq!(format_duration(Duration::from_mins(2)), "2 minutes");
            assert_eq!(
                format_duration(Duration::from_secs(90)),
                "1 minute and 30 seconds"
            );
        }

        #[test]
        fn hours() {
            assert_eq!(format_duration(Duration::from_hours(1)), "1 hour");
            assert_eq!(format_duration(Duration::from_hours(2)), "2 hours");
        }
    }

    mod rate_limiter_manager {
        use super::*;

        fn id(raw: &str) -> ThreemaId {
            ThreemaId::try_from(raw).unwrap()
        }

        fn manager(per_minute: u32, per_hour: u32) -> RateLimiterManager {
            RateLimiterManager::new(RateLimitConfig {
                messages_per_minute: per_minute,
                messages_per_hour: per_hour,
            })
        }

        #[test]
        fn allows_requests_within_limit() {
            let mgr = manager(5, 100);
            let user = id("USER0001");
            for _ in 0_u32..5 {
                assert!(matches!(mgr.check(user), RateLimitResult::Allowed));
            }
        }

        #[test]
        fn blocks_excess_per_minute() {
            let mgr = manager(2, 100);
            let user = id("USER0001");
            assert!(matches!(mgr.check(user), RateLimitResult::Allowed));
            assert!(matches!(mgr.check(user), RateLimitResult::Allowed));
            assert!(matches!(mgr.check(user), RateLimitResult::Limited { .. }));
        }

        #[test]
        fn blocks_excess_per_hour() {
            let mgr = manager(100, 2);
            let user = id("USER0001");
            assert!(matches!(mgr.check(user), RateLimitResult::Allowed));
            assert!(matches!(mgr.check(user), RateLimitResult::Allowed));
            assert!(matches!(mgr.check(user), RateLimitResult::Limited { .. }));
        }

        #[test]
        fn per_user_isolation() {
            let mgr = manager(1, 100);
            let user1 = id("USER0001");
            let user2 = id("USER0002");
            let user3 = id("USER0003");
            assert!(matches!(mgr.check(user1), RateLimitResult::Allowed));
            assert!(matches!(mgr.check(user2), RateLimitResult::Allowed));
            assert!(matches!(mgr.check(user3), RateLimitResult::Allowed));
            assert!(matches!(mgr.check(user1), RateLimitResult::Limited { .. }));
        }
    }
}
