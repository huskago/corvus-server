use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: u64,
    pub iat: u64,
}

pub fn hash_password(password: &str) -> String {
    let salt_bytes: [u8; 16] = rand::random();
    let salt = SaltString::encode_b64(&salt_bytes).expect("encode salt");
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("hash password")
        .to_string()
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub fn generate_jwt_secret() -> String {
    let bytes: [u8; 32] = rand::random();
    hex::encode(bytes)
}

pub fn create_token(
    username: &str,
    secret: &str,
    expiry_secs: u64,
) -> Result<(String, u64), AppError> {
    let now = chrono::Utc::now().timestamp() as u64;
    let exp = now + expiry_secs;
    let claims = Claims { sub: username.to_string(), exp, iat: now };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok((token, exp))
}

pub fn verify_token(token: &str, secret: &str) -> Result<Claims, AppError> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .map(|d| d.claims)
    .map_err(|_| AppError::Unauthorized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_and_verify() {
        let hash = hash_password("secret123");
        assert!(verify_password("secret123", &hash));
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn jwt_create_and_verify() {
        let secret = "test_secret";
        let (token, _) = create_token("admin", secret, 86400).unwrap();
        let claims = verify_token(&token, secret).unwrap();
        assert_eq!(claims.sub, "admin");
    }

    #[test]
    fn jwt_wrong_secret_rejected() {
        let (token, _) = create_token("admin", "secret_a", 86400).unwrap();
        assert!(verify_token(&token, "secret_b").is_err());
    }
}
