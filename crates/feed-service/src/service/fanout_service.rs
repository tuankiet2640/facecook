use std::sync::Arc;

use metrics::counter;
use tracing::{info, instrument, warn};
use uuid::Uuid;

use shared::{
    cache::CacheClient,
    config::FeedConfig,
    errors::{AppError, AppResult},
    kafka::KafkaProducer,
};

use crate::{
    domain::models::FeedItem,
    repository::feed_repo::FeedRepository,
};

/// Hybrid fanout service implementing the celebrity problem solution.
///
/// Strategy:
/// - Regular users (followers <= threshold): FANOUT-ON-WRITE
///   Push post_id to every follower's feed sorted set immediately.
///   Reads are cheap (just retrieve pre-computed feed).
///   Write amplification: O(n) where n = follower count.
///
/// - Celebrity users (followers > threshold): FANOUT-ON-READ
///   Only record the post in a "celebrity posts" index.
///   On read, merge follower's personal feed with followed celebrities' posts.
///   Write is O(1), read is O(k) where k = celebrities followed (typically small).
///
/// This hybrid prevents the hotkey problem where a single post from a user with
/// 10M followers would require 10M Redis writes simultaneously.
pub struct FanoutService {
    feed_repo: Arc<FeedRepository>,
    cache: Arc<CacheClient>,
    config: FeedConfig,
}

impl FanoutService {
    pub fn new(
        feed_repo: Arc<FeedRepository>,
        cache: Arc<CacheClient>,
        config: FeedConfig,
    ) -> Self {
        Self {
            feed_repo,
            cache,
            config,
        }
    }

    /// Entry point called by the fanout worker when a PostCreated event is consumed.
    #[instrument(skip(self), fields(post_id = %post_id, author_id = %author_id))]
    pub async fn fanout_post(
        &self,
        post_id: Uuid,
        author_id: Uuid,
        timestamp_ms: i64,
    ) -> AppResult<()> {
        let start = std::time::Instant::now();
        let timestamp_score = timestamp_ms as f64;

        let follower_count = self.feed_repo.get_follower_count(author_id).await?;

        let use_write_fanout = follower_count <= self.config.celebrity_threshold;

        info!(
            follower_count = follower_count,
            strategy = if use_write_fanout { "fanout-on-write" } else { "fanout-on-read" },
            threshold = self.config.celebrity_threshold,
            "Executing fanout"
        );

        if use_write_fanout {
            self.fanout_on_write(post_id, author_id, timestamp_score, follower_count)
                .await?;
            counter!("feed_fanout_total", "strategy" => "write").increment(1);
        } else {
            self.fanout_on_read(post_id, author_id, timestamp_score).await?;
            counter!("feed_fanout_total", "strategy" => "read").increment(1);
        }

        metrics::histogram!("feed_fanout_duration_seconds")
            .record(start.elapsed().as_secs_f64());

        Ok(())
    }

    /// Fanout-on-write: push post to every follower's feed sorted set.
    ///
    /// Uses batch pagination (1000 followers/batch) + concurrent Redis writes
    /// within each batch to balance throughput vs memory pressure.
    async fn fanout_on_write(
        &self,
        post_id: Uuid,
        author_id: Uuid,
        score: f64,
        follower_count: i64,
    ) -> AppResult<()> {
        const BATCH_SIZE: i64 = 1000;
        let mut offset: i64 = 0;
        let post_member = post_id.to_string();
        let max_feed_size = self.config.max_feed_size;

        loop {
            let follower_ids = self
                .feed_repo
                .get_follower_ids(author_id, offset, BATCH_SIZE)
                .await?;

            if follower_ids.is_empty() {
                break;
            }

            let batch_len = follower_ids.len();

            // Push to all followers in this batch concurrently
            let tasks: Vec<_> = follower_ids
                .into_iter()
                .map(|follower_id| {
                    let cache = Arc::clone(&self.cache);
                    let feed_key = feed_sorted_set_key(follower_id);
                    let member = post_member.clone();

                    async move {
                        // Add post to sorted set (score = timestamp)
                        cache.zadd(&feed_key, score, &member).await?;
                        // Trim to max_feed_size — removes oldest entries
                        // zremrangebyrank(key, 0, -(N+1)) keeps the top N
                        cache
                            .zremrangebyrank(&feed_key, 0, -(max_feed_size as isize + 1))
                            .await?;
                        Ok::<_, AppError>(())
                    }
                })
                .collect();

            futures::future::try_join_all(tasks).await?;

            info!(
                post_id = %post_id,
                batch_size = batch_len,
                offset = offset,
                total_followers = follower_count,
                "Fanout batch written"
            );

            offset += BATCH_SIZE;
            if offset >= follower_count {
                break;
            }
        }

        Ok(())
    }

    /// Fanout-on-read: record post for celebrity, defer distribution to read time.
    ///
    /// O(1) write. The post is indexed by author_id in Redis + persisted to DB.
    /// At read time, merge with personal feed for each requesting user.
    async fn fanout_on_read(
        &self,
        post_id: Uuid,
        author_id: Uuid,
        score: f64,
    ) -> AppResult<()> {
        let key = celebrity_posts_key(author_id);
        let member = post_id.to_string();

        self.cache.zadd(&key, score, &member).await?;
        // Keep last 1000 celebrity posts in cache
        self.cache.zremrangebyrank(&key, 0, -1001).await?;

        // Persist to DB for durability (cache is volatile)
        self.feed_repo
            .record_celebrity_post(post_id, author_id, score as i64)
            .await?;

        Ok(())
    }

    /// Build a user's feed by merging their personal sorted set with celebrity posts.
    ///
    /// Algorithm:
    /// 1. Fetch personal feed (fanout-on-write posts) from Redis sorted set
    /// 2. Identify which followed accounts are celebrities
    /// 3. Fetch those celebrities' recent posts from Redis
    /// 4. Merge all items, apply cursor, sort descending, truncate to limit
    ///
    /// Complexity: O(P + C*K) where P = personal feed size, C = celebrity count, K = posts/celebrity
    #[instrument(skip(self), fields(user_id = %user_id))]
    pub async fn build_feed(
        &self,
        user_id: Uuid,
        cursor: Option<f64>,
        limit: i32,
    ) -> AppResult<Vec<FeedItem>> {
        let feed_key = feed_sorted_set_key(user_id);
        let fetch_count = (limit * 3) as isize; // fetch 3x to account for merging + filtering

        // 1. Get personal feed
        let personal_raw: Vec<(String, f64)> = self
            .cache
            .zrevrange_with_scores(&feed_key, 0, fetch_count)
            .await?;

        let mut all_items: Vec<FeedItem> = personal_raw
            .into_iter()
            .filter_map(|(id_str, score)| {
                Uuid::parse_str(&id_str)
                    .ok()
                    .map(|post_id| FeedItem { post_id, score })
            })
            .collect();

        // 2. Get celebrity followees
        let celebrity_ids = self
            .feed_repo
            .get_followed_celebrities(user_id, self.config.celebrity_threshold)
            .await?;

        // 3. Merge celebrity posts
        for celebrity_id in celebrity_ids {
            let celeb_key = celebrity_posts_key(celebrity_id);
            let celeb_raw: Vec<(String, f64)> = self
                .cache
                .zrevrange_with_scores(&celeb_key, 0, (limit - 1) as isize)
                .await?;

            let celeb_items: Vec<FeedItem> = celeb_raw
                .into_iter()
                .filter_map(|(id_str, score)| {
                    Uuid::parse_str(&id_str)
                        .ok()
                        .map(|post_id| FeedItem { post_id, score })
                })
                .collect();

            all_items.extend(celeb_items);
        }

        // 4. Apply cursor for pagination (score-based: return items older than cursor)
        if let Some(cursor_score) = cursor {
            all_items.retain(|item| item.score < cursor_score);
        }

        // 5. Sort descending by score (newest first), deduplicate, take limit
        all_items.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Deduplicate by post_id (same post could appear from multiple sources)
        all_items.dedup_by_key(|item| item.post_id);

        all_items.truncate(limit as usize);

        counter!("feed_reads_total").increment(1);

        Ok(all_items)
    }
}

/// Redis sorted set key for a user's personal feed.
/// Score = timestamp_ms (f64). Members = post UUIDs.
pub fn feed_sorted_set_key(user_id: Uuid) -> String {
    format!("feed:{}", user_id)
}

/// Redis sorted set key for a celebrity's recent posts.
/// Score = timestamp_ms. Members = post UUIDs.
pub fn celebrity_posts_key(author_id: Uuid) -> String {
    format!("celebrity_posts:{}", author_id)
}
