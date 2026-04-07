use anyhow::Result;
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode, get_current_timestamp,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const TOKEN_SUBJECT: &str = "omnect-ui";
const TOKEN_EXPIRE_HOURS: u64 = 2;
const TOKEN_TIME_TOLERANCE_SECS: u64 = 15 * 60;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    iat: u64,
    exp: u64,
}

/// Centralized token management for session tokens
///
/// Handles creation and verification of JWT tokens used for:
/// - Session authentication
///
/// This struct is cheap to clone (uses Arc internally) and can be safely
/// shared across threads and added to application data.
#[derive(Clone)]
pub struct TokenManager {
    inner: Arc<TokenManagerInner>,
}

struct TokenManagerInner {
    key: Vec<u8>,
}

impl TokenManager {
    /// Create a new `TokenManager`
    ///
    /// # Arguments
    /// * `secret` - Secret key for HMAC-SHA256 signing
    #[must_use]
    pub fn new(secret: &str) -> Self {
        Self {
            inner: Arc::new(TokenManagerInner {
                key: secret.as_bytes().to_vec(),
            }),
        }
    }

    /// Create a new token with the configured expiration and subject
    ///
    /// Returns a signed JWT token string
    pub fn create_token(&self) -> Result<String> {
        let iat = get_current_timestamp();
        let exp = iat + TOKEN_EXPIRE_HOURS * 3600;

        let claims = Claims {
            sub: TOKEN_SUBJECT.to_string(),
            iat,
            exp,
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(&self.inner.key),
        )
        .map_err(|e| anyhow::anyhow!("failed to create token: {e:#}"))
    }

    /// Verify a token and check if it's valid
    ///
    /// Validates:
    /// - Signature
    /// - Expiration (with configurable time tolerance)
    /// - Required subject claim
    ///
    /// Returns true if token is valid, false otherwise
    #[must_use]
    pub fn verify_token(&self, token: &str) -> bool {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.leeway = TOKEN_TIME_TOLERANCE_SECS;
        validation.sub = Some(TOKEN_SUBJECT.to_string());
        validation.validate_exp = true;

        decode::<Claims>(
            token,
            &DecodingKey::from_secret(&self.inner.key),
            &validation,
        )
        .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_verify_token() {
        let manager = TokenManager::new("test-secret-key!");
        let token = manager.create_token().expect("should create token");

        assert!(!token.is_empty());
        assert!(manager.verify_token(&token));
    }

    #[test]
    fn test_verify_invalid_token() {
        let manager = TokenManager::new("test-secret-key!");

        assert!(!manager.verify_token("invalid.token.here"));
        assert!(!manager.verify_token(""));
    }

    #[test]
    fn test_verify_token_wrong_secret() {
        let manager1 = TokenManager::new("first-secret-key!");
        let manager2 = TokenManager::new("other-secret-key!");
        let token = manager1.create_token().expect("should create token");

        // Token created with secret1 should not verify with secret2
        assert!(!manager2.verify_token(&token));
    }
}
