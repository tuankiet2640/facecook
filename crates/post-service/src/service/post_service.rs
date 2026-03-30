use std::sync::Arc;

use tracing::{info, instrument, warn};
use uuid::Uuid;

use shared::{
    cache::CacheClient,
    errors::{AppError, AppResult},
    kafka::{KafkaEvent, KafkaProducer},
    models::post::{Post, PostVisibility},
};

use crate::{
    domain::{
        events::{PostCreated, PostDeleted},
        models::CreatePostRequest,
    },
    repository::post_repo::PostRepository,
};

const POST_CACHE_TTL: u64 = 3600; // 1 hour

pub struct PostService {
    repo: Arc<PostRepository>,
    cache: Arc<CacheClient>,
    kafka: Arc<KafkaProducer>,
    post_events_topic: String,
    feed_fanout_topic: String,
}

impl PostService {
    pub fn new(
        repo: Arc<PostRepository>,
        cache: Arc<CacheClient>,
        kafka: Arc<KafkaProducer>,
        post_events_topic: String,
        feed_fanout_topic: String,
    ) -> Self {
        Self {
            repo,
            cache,
            kafka,
            post_events_topic,
            feed_fanout_topic,
        }
    }

    /// Create a post:
    /// 1. Persist to PostgreSQL (durable write)
    /// 2. Cache the post object in Redis (fast reads)
    /// 3. Publish PostCreated event to Kafka (triggers feed fanout asynchronously)
    ///
    /// The DB write is the source of truth. Cache and Kafka are best-effort.
    /// Feed fanout is async — followers may see a slight delay (< 1s typical).
    #[instrument(skip(self, req), fields(author_id = %author_id))]
    pub async fn create_post(
        &self,
        author_id: Uuid,
        req: CreatePostRequest,
    ) -> AppResult<Post> {
        let visibility = req.visibility.unwrap_or(PostVisibility::Public);
        let media_urls = serde_json::json!(req.media_urls);

        let post = self
            .repo
            .create(author_id, &req.content, &media_urls, &visibility)
            .await?;

        // Cache immediately for fast reads by the author and others
        let cache_key = post_cache_key(post.id);
        if let Err(e) = self.cache.set(&cache_key, &post, POST_CACHE_TTL).await {
            warn!(error = %e, post_id = %post.id, "Failed to cache post");
        }

        // Publish to Kafka — feed-service consumer triggers fan-out
        // Using post_id as Kafka key ensures same-post events go to same partition
        // (important for ordering guarantees within a post's lifecycle)
        let timestamp_ms = post.created_at.timestamp_millis();
        let event = KafkaEvent::new(
            "post.created",
            PostCreated {
                post_id: post.id,
                author_id,
                timestamp_ms,
            },
        );

        if let Err(e) = self
            .kafka
            .publish(&self.post_events_topic, &post.id.to_string(), &event)
            .await
        {
            // Non-fatal: post is persisted. Feed may lag but will catch up.
            // In production: add to outbox table for guaranteed delivery.
            warn!(
                error = %e,
                post_id = %post.id,
                "Failed to publish PostCreated event — feed fanout delayed"
            );
        } else {
            metrics::counter!("kafka_events_produced_total", "event_type" => "post.created")
                .increment(1);
        }

        info!(post_id = %post.id, author_id = %author_id, "Post created");
        Ok(post)
    }

    pub async fn get_post(&self, post_id: Uuid) -> AppResult<Post> {
        let cache_key = post_cache_key(post_id);

        // Cache-aside pattern
        if let Ok(Some(cached)) = self.cache.get::<Post>(&cache_key).await {
            return Ok(cached);
        }

        let post = self
            .repo
            .find_by_id(post_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Post {} not found", post_id)))?;

        let _ = self.cache.set(&cache_key, &post, POST_CACHE_TTL).await;
        Ok(post)
    }

    pub async fn get_user_posts(
        &self,
        author_id: Uuid,
        limit: i64,
        before_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    ) -> AppResult<Vec<Post>> {
        self.repo
            .get_user_posts(author_id, limit, before_timestamp)
            .await
    }

    /// Batch fetch posts by IDs. Used by feed-service when hydrating a feed.
    /// Checks cache first, only fetches misses from DB.
    pub async fn get_posts_by_ids(&self, post_ids: Vec<Uuid>) -> AppResult<Vec<Post>> {
        let mut cached_posts: Vec<Post> = Vec::new();
        let mut missing_ids: Vec<Uuid> = Vec::new();

        for &post_id in &post_ids {
            let cache_key = post_cache_key(post_id);
            match self.cache.get::<Post>(&cache_key).await {
                Ok(Some(post)) => cached_posts.push(post),
                _ => missing_ids.push(post_id),
            }
        }

        if !missing_ids.is_empty() {
            let db_posts = self.repo.get_posts_by_ids(&missing_ids).await?;

            // Back-fill cache for future reads
            for post in &db_posts {
                let cache_key = post_cache_key(post.id);
                let _ = self.cache.set(&cache_key, post, POST_CACHE_TTL).await;
            }

            cached_posts.extend(db_posts);
        }

        // Preserve original ordering
        let id_order: std::collections::HashMap<Uuid, usize> = post_ids
            .iter()
            .enumerate()
            .map(|(i, &id)| (id, i))
            .collect();

        cached_posts.sort_by_key(|p| id_order.get(&p.id).copied().unwrap_or(usize::MAX));

        Ok(cached_posts)
    }

    #[instrument(skip(self), fields(post_id = %post_id, requester_id = %requester_id))]
    pub async fn delete_post(&self, post_id: Uuid, requester_id: Uuid) -> AppResult<()> {
        let deleted = self.repo.delete(post_id, requester_id).await?;

        if !deleted {
            return Err(AppError::NotFound(format!(
                "Post {} not found or not owned by requester",
                post_id
            )));
        }

        // Invalidate cache
        let _ = self.cache.del(&post_cache_key(post_id)).await;

        // Notify feed-service to remove from feeds
        let event = KafkaEvent::new(
            "post.deleted",
            PostDeleted {
                post_id,
                author_id: requester_id,
            },
        );

        if let Err(e) = self
            .kafka
            .publish(&self.post_events_topic, &post_id.to_string(), &event)
            .await
        {
            warn!(error = %e, post_id = %post_id, "Failed to publish PostDeleted event");
        }

        info!(post_id = %post_id, "Post deleted");
        Ok(())
    }
}

fn post_cache_key(post_id: Uuid) -> String {
    format!("post:{}", post_id)
}
