use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::AppError;

/// JWT claims embedded in every access token.
/// jti enables token revocation by tracking invalidated IDs in Redis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,       // user_id (standard JWT subject)
    pub username: String,
    pub email: String,
    pub iat: i64,        // issued-at (unix timestamp)
    pub exp: i64,        // expiry (unix timestamp)
    pub jti: String,     // JWT ID for revocation tracking
}

/// Authenticated user context extracted from validated JWT.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub username: String,
    pub email: String,
}

impl From<Claims> for AuthUser {
    fn from(claims: Claims) -> Self {
        Self {
            user_id: claims.sub,
            username: claims.username,
            email: claims.email,
        }
    }
}

/// Stateless JWT service. Keys are derived from the secret at construction time.
pub struct JwtService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    expiry_secs: u64,
    validation: Validation,
}

impl std::fmt::Debug for JwtService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtService")
            .field("expiry_secs", &self.expiry_secs)
            .finish_non_exhaustive()
    }
}

impl JwtService {
    pub fn new(secret: &str, expiry_secs: u64) -> Self {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;

        Self {
            encoding_key: EncodingKey::from_secret(secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(secret.as_bytes()),
            expiry_secs,
            validation,
        }
    }

    /// Issue a signed JWT for a user. Call after successful authentication.
    pub fn issue_token(
        &self,
        user_id: Uuid,
        username: &str,
        email: &str,
    ) -> Result<String, AppError> {
        let now = chrono::Utc::now().timestamp();
        let claims = Claims {
            sub: user_id,
            username: username.to_string(),
            email: email.to_string(),
            iat: now,
            exp: now + self.expiry_secs as i64,
            jti: Uuid::new_v4().to_string(),
        };

        encode(&Header::default(), &claims, &self.encoding_key)
            .map_err(|e| AppError::Unauthorized(e.to_string()))
    }

    pub fn expiry_secs(&self) -> u64 {
        self.expiry_secs
    }

    /// Validate a JWT string and return its claims if valid.
    pub fn validate_token(&self, token: &str) -> Result<Claims, AppError> {
        decode::<Claims>(token, &self.decoding_key, &self.validation)
            .map(|data| data.claims)
            .map_err(|e| AppError::Unauthorized(format!("Invalid token: {}", e)))
    }
}

/// Extract a bearer token from the Authorization header value.
pub fn extract_bearer_token(header_value: &str) -> Option<&str> {
    header_value.strip_prefix("Bearer ").map(str::trim)
}

/// Extract a token from a WebSocket connection's query string.
/// WS clients cannot set custom headers, so token is passed as ?token=...
pub fn extract_token_from_query(query: &str) -> Option<String> {
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(k, _)| k == "token")
        .map(|(_, v)| v.to_string())
}
