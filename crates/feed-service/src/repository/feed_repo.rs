use uuid::Uuid;

use shared::{db::DbPool, errors::AppResult};

pub struct FeedRepository {
    pool: DbPool,
}

impl FeedRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    /// Total follower count — used to determine fanout strategy.
    pub async fn get_follower_count(&self, user_id: Uuid) -> AppResult<i64> {
        let row = sqlx::query!("SELECT follower_count FROM users WHERE id = $1", user_id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|r| r.follower_count).unwrap_or(0))
    }

    /// Paginated follower IDs for fanout-on-write batching.
    /// `cursor` is an offset into the follower list.
    pub async fn get_follower_ids(
        &self,
        user_id: Uuid,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<Uuid>> {
        let rows = sqlx::query!(
            r#"
            SELECT follower_id
            FROM follows
            WHERE followee_id = $1
            ORDER BY created_at ASC
            LIMIT $2 OFFSET $3
            "#,
            user_id,
            limit,
            offset
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.follower_id).collect())
    }

    /// Celebrity IDs that a given user follows.
    /// Used during fanout-on-read to merge celebrity posts into the feed.
    pub async fn get_followed_celebrities(
        &self,
        user_id: Uuid,
        celebrity_threshold: i64,
    ) -> AppResult<Vec<Uuid>> {
        let rows = sqlx::query!(
            r#"
            SELECT f.followee_id
            FROM follows f
            JOIN users u ON u.id = f.followee_id
            WHERE f.follower_id = $1
              AND u.follower_count > $2
            "#,
            user_id,
            celebrity_threshold,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.followee_id).collect())
    }

    /// Store celebrity post reference for durability.
    /// Redis entry may be evicted; DB is the authoritative source.
    pub async fn record_celebrity_post(
        &self,
        post_id: Uuid,
        author_id: Uuid,
        _timestamp_ms: i64,
    ) -> AppResult<()> {
        sqlx::query!(
            r#"
            INSERT INTO celebrity_posts (post_id, author_id)
            VALUES ($1, $2)
            ON CONFLICT DO NOTHING
            "#,
            post_id,
            author_id,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Fetch celebrity posts from DB when Redis cache is cold.
    pub async fn get_celebrity_posts_since(
        &self,
        author_id: Uuid,
        since_ms: i64,
        limit: i64,
    ) -> AppResult<Vec<(Uuid, i64)>> {
        let since = chrono::DateTime::from_timestamp_millis(since_ms)
            .unwrap_or_default()
            .with_timezone(&chrono::Utc);

        let rows = sqlx::query!(
            r#"
            SELECT cp.post_id, EXTRACT(EPOCH FROM cp.created_at) * 1000 AS "timestamp_ms: i64"
            FROM celebrity_posts cp
            WHERE cp.author_id = $1
              AND cp.created_at >= $2
            ORDER BY cp.created_at DESC
            LIMIT $3
            "#,
            author_id,
            since,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| (r.post_id, r.timestamp_ms.unwrap_or(0)))
            .collect())
    }
}
