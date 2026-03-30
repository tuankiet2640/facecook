use uuid::Uuid;

use shared::{
    db::DbPool,
    errors::AppResult,
    models::post::{Post, PostVisibility},
};

pub struct PostRepository {
    pool: DbPool,
}

impl PostRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        author_id: Uuid,
        content: &str,
        media_urls: &serde_json::Value,
        visibility: &PostVisibility,
    ) -> AppResult<Post> {
        let post = sqlx::query_as!(
            Post,
            r#"
            INSERT INTO posts (author_id, content, media_urls, visibility)
            VALUES ($1, $2, $3, $4)
            RETURNING id, author_id, content, media_urls,
                      like_count, comment_count, share_count,
                      visibility AS "visibility: PostVisibility",
                      created_at, updated_at
            "#,
            author_id,
            content,
            media_urls,
            visibility as &PostVisibility,
        )
        .fetch_one(&self.pool)
        .await?;

        // Increment post_count atomically
        sqlx::query!(
            "UPDATE users SET post_count = post_count + 1 WHERE id = $1",
            author_id
        )
        .execute(&self.pool)
        .await?;

        Ok(post)
    }

    pub async fn find_by_id(&self, post_id: Uuid) -> AppResult<Option<Post>> {
        let post = sqlx::query_as!(
            Post,
            r#"
            SELECT id, author_id, content, media_urls,
                   like_count, comment_count, share_count,
                   visibility AS "visibility: PostVisibility",
                   created_at, updated_at
            FROM posts
            WHERE id = $1
            "#,
            post_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(post)
    }

    pub async fn get_user_posts(
        &self,
        author_id: Uuid,
        limit: i64,
        before_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    ) -> AppResult<Vec<Post>> {
        let posts = if let Some(before) = before_timestamp {
            sqlx::query_as!(
                Post,
                r#"
                SELECT id, author_id, content, media_urls,
                       like_count, comment_count, share_count,
                       visibility AS "visibility: PostVisibility",
                       created_at, updated_at
                FROM posts
                WHERE author_id = $1 AND created_at < $2
                  AND visibility = 'public'
                ORDER BY created_at DESC
                LIMIT $3
                "#,
                author_id,
                before,
                limit
            )
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as!(
                Post,
                r#"
                SELECT id, author_id, content, media_urls,
                       like_count, comment_count, share_count,
                       visibility AS "visibility: PostVisibility",
                       created_at, updated_at
                FROM posts
                WHERE author_id = $1 AND visibility = 'public'
                ORDER BY created_at DESC
                LIMIT $2
                "#,
                author_id,
                limit
            )
            .fetch_all(&self.pool)
            .await?
        };

        Ok(posts)
    }

    pub async fn get_posts_by_ids(&self, post_ids: &[Uuid]) -> AppResult<Vec<Post>> {
        let posts = sqlx::query_as!(
            Post,
            r#"
            SELECT id, author_id, content, media_urls,
                   like_count, comment_count, share_count,
                   visibility AS "visibility: PostVisibility",
                   created_at, updated_at
            FROM posts
            WHERE id = ANY($1)
            ORDER BY created_at DESC
            "#,
            post_ids
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(posts)
    }

    pub async fn delete(&self, post_id: Uuid, author_id: Uuid) -> AppResult<bool> {
        let result = sqlx::query!(
            "DELETE FROM posts WHERE id = $1 AND author_id = $2",
            post_id,
            author_id
        )
        .execute(&self.pool)
        .await?;

        if result.rows_affected() > 0 {
            sqlx::query!(
                "UPDATE users SET post_count = GREATEST(post_count - 1, 0) WHERE id = $1",
                author_id
            )
            .execute(&self.pool)
            .await?;
        }

        Ok(result.rows_affected() > 0)
    }

    pub async fn increment_like(&self, post_id: Uuid) -> AppResult<()> {
        sqlx::query!(
            "UPDATE posts SET like_count = like_count + 1 WHERE id = $1",
            post_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn decrement_like(&self, post_id: Uuid) -> AppResult<()> {
        sqlx::query!(
            "UPDATE posts SET like_count = GREATEST(like_count - 1, 0) WHERE id = $1",
            post_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
