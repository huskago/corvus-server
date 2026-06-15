use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::net::SocketAddr;

use crate::{
    auth::{create_token, generate_jwt_secret, hash_password, verify_password},
    error::AppError,
    models::{LoginRequest, LoginResponse},
    state::AppState,
};

pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    state.check_login_rate_limit(addr.ip())?;

    let (username, password_hash, jwt_secret, jwt_expiry_secs) = {
        let cfg = state.config.read().await;
        (
            cfg.auth.username.clone(),
            cfg.auth.password_hash.clone(),
            cfg.auth.jwt_secret.clone(),
            cfg.auth.jwt_expiry_secs,
        )
    };

    if body.username != username || !verify_password(&body.password, &password_hash) {
        return Err(AppError::Unauthorized);
    }

    state.reset_login_attempts(addr.ip());

    let (token, expires_at) = create_token(&body.username, &jwt_secret, jwt_expiry_secs)?;
    Ok(Json(LoginResponse { token, expires_at }))
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    _auth: crate::routes::AuthUser,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<StatusCode, AppError> {
    let current_hash = state.config.read().await.auth.password_hash.clone();

    if !verify_password(&body.current_password, &current_hash) {
        return Err(AppError::Unauthorized);
    }
    if body.new_password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }

    // Hash before acquiring the write lock (Argon2 is intentionally slow).
    let new_hash = hash_password(&body.new_password);
    let new_secret = generate_jwt_secret();

    let mut new_cfg = state.config.read().await.clone();
    new_cfg.auth.password_hash = new_hash;
    new_cfg.auth.jwt_secret = new_secret;

    // Persist first, if disk write fails, in-memory config stays unchanged.
    new_cfg
        .save_to(&state.config_path)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    *state.config.write().await = new_cfg;

    Ok(StatusCode::NO_CONTENT)
}
