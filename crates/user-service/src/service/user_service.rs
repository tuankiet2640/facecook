use std::sync::Arc;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use tracing::{info, instrument, warn};
use uuid::Uuid;

use shared::{
    cache::CacheClient,
    errors::{AppError, AppResult},
    kafka::{KafkaEvent, KafkaProducer},
    models::user::{User, UserProfile},
};

use crate::{
    domain::{
        events::{UserFollowed, UserRegistered, UserUnfollowed},
        models::{AuthResponse, LoginRequest, RegisterRequest, UpdateProfileRequest},
    },
    repository::user_repo::UserRepository,
};

const USER_CACHE_TTL: u64 = 300; // 5 minutes

pub struct UserService {
    repo: Arc<UserRepository>,
    cache: Arc<CacheClient>,
    kafka: Arc<KafkaProducer>,
    notification_topic: String,
}

impl UserService {
    pub fn new(
        repo: Arc<UserRepository>,
        cache: Arc<CacheClient>,
        kafka: Arc<KafkaProducer>,
        notification_topic: String,
    ) -> Self {
        Self {
            repo,
            cache,
            kafka,
            notification_topic,
        }
    }

    /// Register a new user with Argon2id password hashing.
    ///
    /// Argon2id is the recommended algorithm per OWASP:
    /// - memory-hard: resists GPU/ASIC attacks
    /// - time parameter: configurable work factor
    /// - Uses OS random salt: unique per password
    #[instrument(skip(self, req), fields(username = %req.username, email = %req.email))]
    pub async fn register(&self, req: RegisterRequest) -> AppResult<(User, String)> {
        // Check uniqueness before hashing (cheaper)
        if self.repo.find_by_email(&req.email).await?.is_some() {
            return Err(AppError::Conflict(
                "Email address already registered".to_string(),
            ));
        }
        if self.repo.find_by_username(&req.username).await?.is_some() {
            return Err(AppError::Conflict("Username already taken".to_string()));
        }

        // Hash password with Argon2id (blocking — run on thread pool)
        let password = req.password.clone();
        let password_hash = tokio::task::spawn_blocking(move || {
            let salt = SaltString::generate(&mut OsRng);
            let argon2 = Argon2::default();
            argon2
                .hash_password(password.as_bytes(), &salt)
                .map(|h| h.to_string())
                .map_err(|e| AppError::Internal(anyhow::anyhow!("Password hashing failed: {}", e)))
        })
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Thread panic: {}", e)))??;

        let user = self
            .repo
            .create(&req.username, &req.email, &password_hash, &req.display_name)
            .await?;

        // Publish registration event (non-blocking, fire-and-forget)
        let event = KafkaEvent::new(
            "user.registered",
            UserRegistered {
                user_id: user.id,
                username: user.username.clone(),
                email: user.email.clone(),
            },
        );

        if let Err(e) = self
            .kafka
            .publish(&self.notification_topic, &user.id.to_string(), &event)
            .await
        {
            // Non-fatal: user is created, just event emission failed
            warn!(error = %e, user_id = %user.id, "Failed to publish UserRegistered event");
        }

        info!(user_id = %user.id, username = %user.username, "User registered");

        // Cache the new user profile
        let cache_key = user_cache_key(user.id);
        let profile = UserProfile::from(user.clone());
        let _ = self.cache.set(&cache_key, &profile, USER_CACHE_TTL).await;

        Ok((user, password_hash))
    }

    /// Authenticate a user and return a JWT access token.
    #[instrument(skip(self, req, jwt_service), fields(email = %req.email))]
    pub async fn login(
        &self,
        req: LoginRequest,
        jwt_service: &shared::auth::JwtService,
    ) -> AppResult<AuthResponse> {
        let (user, stored_hash) = self
            .repo
            .find_by_email(&req.email)
            .await?
            .ok_or_else(|| AppError::Unauthorized("Invalid credentials".to_string()))?;

        // Verify password on blocking thread (argon2 is CPU-intensive)
        let password = req.password.clone();
        let hash = stored_hash.clone();
        let valid = tokio::task::spawn_blocking(move || {
            let parsed = PasswordHash::new(&hash)
                .map_err(|e| AppError::Internal(anyhow::anyhow!("Hash parse error: {}", e)))?;
            Ok::<bool, AppError>(
                Argon2::default()
                    .verify_password(password.as_bytes(), &parsed)
                    .is_ok(),
            )
        })
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Thread panic: {}", e)))??;

        if !valid {
            warn!(email = %req.email, "Failed login attempt");
            return Err(AppError::Unauthorized("Invalid credentials".to_string()));
        }

        let token = jwt_service.issue_token(user.id, &user.username, &user.email)?;

        info!(user_id = %user.id, "User logged in");

        Ok(AuthResponse {
            access_token: token,
            token_type: "Bearer".to_string(),
            expires_in: jwt_service.expiry_secs(),
            user_id: user.id,
            username: user.username,
        })
    }

    pub async fn get_profile(&self, user_id: Uuid) -> AppResult<UserProfile> {
        let cache_key = user_cache_key(user_id);

        // Cache-aside: check Redis first
        if let Ok(Some(cached)) = self.cache.get::<UserProfile>(&cache_key).await {
            return Ok(cached);
        }

        let user = self
            .repo
            .find_by_id(user_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("User {} not found", user_id)))?;

        let profile = UserProfile::from(user);
        let _ = self.cache.set(&cache_key, &profile, USER_CACHE_TTL).await;

        Ok(profile)
    }

    pub async fn get_profile_by_username(&self, username: &str) -> AppResult<UserProfile> {
        let user = self
            .repo
            .find_by_username(username)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("User @{} not found", username)))?;

        Ok(UserProfile::from(user))
    }

    #[instrument(skip(self, req))]
    pub async fn update_profile(
        &self,
        user_id: Uuid,
        req: UpdateProfileRequest,
    ) -> AppResult<UserProfile> {
        let user = self
            .repo
            .update_profile(
                user_id,
                req.display_name.as_deref(),
                req.bio.as_deref(),
                req.avatar_url.as_deref(),
            )
            .await?;

        // Invalidate cache entry
        let cache_key = user_cache_key(user_id);
        let _ = self.cache.del(&cache_key).await;

        Ok(UserProfile::from(user))
    }

    /// Follow another user.
    /// Atomic DB transaction ensures follow row and counters stay consistent.
    #[instrument(skip(self), fields(follower_id = %follower_id, followee_id = %followee_id))]
    pub async fn follow(&self, follower_id: Uuid, followee_id: Uuid) -> AppResult<()> {
        if follower_id == followee_id {
            return Err(AppError::BadRequest("Cannot follow yourself".to_string()));
        }

        // Verify followee exists
        self.repo
            .find_by_id(followee_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("User {} not found", followee_id)))?;

        self.repo.follow(follower_id, followee_id).await?;

        // Invalidate both users' cached profiles (follower/following counts changed)
        let _ = self.cache.del(&user_cache_key(follower_id)).await;
        let _ = self.cache.del(&user_cache_key(followee_id)).await;

        // Publish follow event for feed service to react to
        let event = KafkaEvent::new(
            "user.followed",
            UserFollowed {
                follower_id,
                followee_id,
            },
        );

        if let Err(e) = self
            .kafka
            .publish(&self.notification_topic, &follower_id.to_string(), &event)
            .await
        {
            warn!(error = %e, "Failed to publish UserFollowed event");
        }

        info!(follower_id = %follower_id, followee_id = %followee_id, "Follow created");
        Ok(())
    }

    /// Unfollow a user.
    #[instrument(skip(self))]
    pub async fn unfollow(&self, follower_id: Uuid, followee_id: Uuid) -> AppResult<()> {
        self.repo.unfollow(follower_id, followee_id).await?;

        let _ = self.cache.del(&user_cache_key(follower_id)).await;
        let _ = self.cache.del(&user_cache_key(followee_id)).await;

        let event = KafkaEvent::new(
            "user.unfollowed",
            UserUnfollowed {
                follower_id,
                followee_id,
            },
        );

        if let Err(e) = self
            .kafka
            .publish(&self.notification_topic, &follower_id.to_string(), &event)
            .await
        {
            warn!(error = %e, "Failed to publish UserUnfollowed event");
        }

        info!(follower_id = %follower_id, followee_id = %followee_id, "Follow removed");
        Ok(())
    }

    pub async fn get_followers(
        &self,
        user_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> AppResult<Vec<UserProfile>> {
        let users = self.repo.get_followers(user_id, limit, offset).await?;
        Ok(users.into_iter().map(UserProfile::from).collect())
    }

    pub async fn get_following(
        &self,
        user_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> AppResult<Vec<UserProfile>> {
        let users = self.repo.get_following(user_id, limit, offset).await?;
        Ok(users.into_iter().map(UserProfile::from).collect())
    }
}

fn user_cache_key(user_id: Uuid) -> String {
    format!("user:profile:{}", user_id)
}
