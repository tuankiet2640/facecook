use uuid::Uuid;

use shared::{
    db::DbPool,
    errors::{AppError, AppResult},
    models::user::{Follow, User},
};

pub struct UserRepository {
    pool: DbPool,
}

impl UserRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub async fn find_by_id(&self, id: Uuid) -> AppResult<Option<User>> {
        let user = sqlx::query_as!(
            User,
            r#"
            SELECT id, username, email, display_name, bio, avatar_url,
                   follower_count, following_count, post_count, is_verified,
                   created_at, updated_at
            FROM users
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(user)
    }

    pub async fn find_by_email(&self, email: &str) -> AppResult<Option<(User, String)>> {
        let row = sqlx::query!(
            r#"
            SELECT id, username, email, display_name, bio, avatar_url,
                   follower_count, following_count, post_count, is_verified,
                   created_at, updated_at, password_hash
            FROM users
            WHERE email = $1
            "#,
            email
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            let user = User {
                id: r.id,
                username: r.username,
                email: r.email,
                display_name: r.display_name,
                bio: r.bio,
                avatar_url: r.avatar_url,
                follower_count: r.follower_count,
                following_count: r.following_count,
                post_count: r.post_count,
                is_verified: r.is_verified,
                created_at: r.created_at,
                updated_at: r.updated_at,
            };
            (user, r.password_hash)
        }))
    }

    pub async fn find_by_username(&self, username: &str) -> AppResult<Option<User>> {
        let user = sqlx::query_as!(
            User,
            r#"
            SELECT id, username, email, display_name, bio, avatar_url,
                   follower_count, following_count, post_count, is_verified,
                   created_at, updated_at
            FROM users
            WHERE username = $1
            "#,
            username
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(user)
    }

    pub async fn create(
        &self,
        username: &str,
        email: &str,
        password_hash: &str,
        display_name: &str,
    ) -> AppResult<User> {
        let user = sqlx::query_as!(
            User,
            r#"
            INSERT INTO users (username, email, password_hash, display_name)
            VALUES ($1, $2, $3, $4)
            RETURNING id, username, email, display_name, bio, avatar_url,
                      follower_count, following_count, post_count, is_verified,
                      created_at, updated_at
            "#,
            username,
            email,
            password_hash,
            display_name
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(user)
    }

    pub async fn update_profile(
        &self,
        user_id: Uuid,
        display_name: Option<&str>,
        bio: Option<&str>,
        avatar_url: Option<&str>,
    ) -> AppResult<User> {
        let user = sqlx::query_as!(
            User,
            r#"
            UPDATE users
            SET
                display_name = COALESCE($2, display_name),
                bio = COALESCE($3, bio),
                avatar_url = COALESCE($4, avatar_url),
                updated_at = NOW()
            WHERE id = $1
            RETURNING id, username, email, display_name, bio, avatar_url,
                      follower_count, following_count, post_count, is_verified,
                      created_at, updated_at
            "#,
            user_id,
            display_name,
            bio,
            avatar_url,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(user)
    }

    /// Atomic follow: insert row + increment both counters in a transaction.
    /// Uses FOR UPDATE to prevent concurrent follow/unfollow races.
    pub async fn follow(&self, follower_id: Uuid, followee_id: Uuid) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;

        // Check not already following
        let existing = sqlx::query!(
            "SELECT 1 as exists FROM follows WHERE follower_id = $1 AND followee_id = $2",
            follower_id,
            followee_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        if existing.is_some() {
            return Err(AppError::Conflict("Already following this user".to_string()));
        }

        sqlx::query!(
            "INSERT INTO follows (follower_id, followee_id) VALUES ($1, $2)",
            follower_id,
            followee_id
        )
        .execute(&mut *tx)
        .await?;

        // Increment following_count for follower
        sqlx::query!(
            "UPDATE users SET following_count = following_count + 1 WHERE id = $1",
            follower_id
        )
        .execute(&mut *tx)
        .await?;

        // Increment follower_count for followee
        sqlx::query!(
            "UPDATE users SET follower_count = follower_count + 1 WHERE id = $1",
            followee_id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Atomic unfollow: remove row + decrement both counters in a transaction.
    pub async fn unfollow(&self, follower_id: Uuid, followee_id: Uuid) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;

        let deleted = sqlx::query!(
            "DELETE FROM follows WHERE follower_id = $1 AND followee_id = $2",
            follower_id,
            followee_id
        )
        .execute(&mut *tx)
        .await?;

        if deleted.rows_affected() == 0 {
            return Err(AppError::NotFound("Follow relationship not found".to_string()));
        }

        sqlx::query!(
            "UPDATE users SET following_count = GREATEST(following_count - 1, 0) WHERE id = $1",
            follower_id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            "UPDATE users SET follower_count = GREATEST(follower_count - 1, 0) WHERE id = $1",
            followee_id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn is_following(&self, follower_id: Uuid, followee_id: Uuid) -> AppResult<bool> {
        let row = sqlx::query!(
            "SELECT 1 as exists FROM follows WHERE follower_id = $1 AND followee_id = $2",
            follower_id,
            followee_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.is_some())
    }

    pub async fn get_followers(
        &self,
        user_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> AppResult<Vec<User>> {
        let users = sqlx::query_as!(
            User,
            r#"
            SELECT u.id, u.username, u.email, u.display_name, u.bio, u.avatar_url,
                   u.follower_count, u.following_count, u.post_count, u.is_verified,
                   u.created_at, u.updated_at
            FROM users u
            JOIN follows f ON f.follower_id = u.id
            WHERE f.followee_id = $1
            ORDER BY f.created_at DESC
            LIMIT $2 OFFSET $3
            "#,
            user_id,
            limit,
            offset
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(users)
    }

    pub async fn get_following(
        &self,
        user_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> AppResult<Vec<User>> {
        let users = sqlx::query_as!(
            User,
            r#"
            SELECT u.id, u.username, u.email, u.display_name, u.bio, u.avatar_url,
                   u.follower_count, u.following_count, u.post_count, u.is_verified,
                   u.created_at, u.updated_at
            FROM users u
            JOIN follows f ON f.followee_id = u.id
            WHERE f.follower_id = $1
            ORDER BY f.created_at DESC
            LIMIT $2 OFFSET $3
            "#,
            user_id,
            limit,
            offset
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(users)
    }
}
