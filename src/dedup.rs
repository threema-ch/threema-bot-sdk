//! Message ID deduplication.

use threema_gateway::protocol::{MessageId, ThreemaId};

/// Result of [`MessageDeduplicator::check_and_insert`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeduplicateResult {
    /// The message ID was not seen before; it has been inserted.
    New,
    /// The message ID was already seen; it is a duplicate.
    Duplicate,
}

/// Message deduplication tracker.
///
/// Uses a TTL cache to prevent bugs and replay attacks by tracking recently
/// seen (sender, message ID) pairs.
pub(crate) struct MessageDeduplicator {
    cache: moka::sync::Cache<(ThreemaId, MessageId), ()>,
}

impl MessageDeduplicator {
    /// Create a new deduplicator with the given capacity and TTL.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of entries to track
    /// * `ttl_seconds` - Time-to-live for entries in seconds
    pub(crate) fn new(capacity: u64, ttl_seconds: u64) -> Self {
        let cache = moka::sync::Cache::builder()
            .time_to_live(std::time::Duration::from_secs(ttl_seconds))
            .max_capacity(capacity)
            .build();

        Self { cache }
    }

    /// Check whether a message has been seen before and insert it if not.
    ///
    /// Keyed on (sender ID, message ID) since message IDs are assigned by
    /// the sender and may collide across different senders.
    ///
    /// This method is atomic – concurrent calls on the same key are coalesced,
    /// so exactly one caller receives [`DeduplicateResult::New`].
    pub(crate) fn check_and_insert(
        &self,
        sender: ThreemaId,
        message_id: MessageId,
    ) -> DeduplicateResult {
        if self
            .cache
            .entry((sender, message_id))
            .or_insert_with(|| ())
            .is_fresh()
        {
            DeduplicateResult::New
        } else {
            DeduplicateResult::Duplicate
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, thread};

    use super::*;

    fn sender(id: &str) -> ThreemaId {
        ThreemaId::try_from(id).unwrap()
    }

    mod check_and_insert {
        use super::*;

        #[test]
        fn first_insert_is_new() {
            let dedup = MessageDeduplicator::new(1000, 300);
            assert_eq!(
                dedup.check_and_insert(sender("SENDER01"), MessageId::from_u64(1)),
                DeduplicateResult::New
            );
        }

        #[test]
        fn second_insert_is_duplicate() {
            let dedup = MessageDeduplicator::new(1000, 300);
            let id = MessageId::from_u64(1);
            dedup.check_and_insert(sender("SENDER01"), id);
            assert_eq!(
                dedup.check_and_insert(sender("SENDER01"), id),
                DeduplicateResult::Duplicate
            );
        }

        #[test]
        fn different_ids_are_independent() {
            let dedup = MessageDeduplicator::new(1000, 300);
            assert_eq!(
                dedup.check_and_insert(sender("SENDER01"), MessageId::from_u64(1)),
                DeduplicateResult::New
            );
            assert_eq!(
                dedup.check_and_insert(sender("SENDER01"), MessageId::from_u64(2)),
                DeduplicateResult::New
            );
        }

        #[test]
        fn same_id_different_senders_are_independent() {
            let dedup = MessageDeduplicator::new(1000, 300);
            let id = MessageId::from_u64(1);
            assert_eq!(
                dedup.check_and_insert(sender("SENDER01"), id),
                DeduplicateResult::New
            );
            assert_eq!(
                dedup.check_and_insert(sender("SENDER02"), id),
                DeduplicateResult::New
            );
        }

        #[test]
        fn concurrent_access() {
            let dedup = Arc::new(MessageDeduplicator::new(10000, 300));
            let sender = sender("SENDER01");
            let mut handles = vec![];

            // Spawn multiple threads that try to insert the same (sender, message ID) pairs
            for thread_id in 0_u32..10 {
                let dedup_clone = Arc::clone(&dedup);
                let handle = thread::spawn(move || {
                    let mut new_count: u32 = 0;
                    for msg_num in 0_u64..100 {
                        if dedup_clone.check_and_insert(sender, MessageId::from_u64(msg_num))
                            == DeduplicateResult::New
                        {
                            new_count = new_count.saturating_add(1);
                        }
                    }
                    (thread_id, new_count)
                });
                handles.push(handle);
            }

            // Collect results
            let mut total_new: u32 = 0;
            for handle in handles {
                let (_thread_id, new_count) = handle.join().unwrap();
                total_new = total_new.saturating_add(new_count);
            }

            // Each unique (sender, message ID) pair should only be counted as
            // "new" once across all threads, so total should be exactly 100
            assert_eq!(total_new, 100_u32);
        }
    }
}
