pub mod auth;
pub mod dashboard;
pub mod extra_files;
pub mod instances;
pub mod manifest;
pub mod news;
pub mod public;
pub mod updates;

use axum::{
    extract::{DefaultBodyLimit, FromRequestParts},
    http::{request::Parts, Method},
    Router,
};
use tower_http::cors::CorsLayer;

use crate::{error::AppError, state::AppState};

pub struct AuthUser;

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(AppError::Unauthorized)?;

        let secret = state.config.read().await.auth.jwt_secret.clone();
        crate::auth::verify_token(token, &secret)?;
        Ok(AuthUser)
    }
}

pub fn build_router(state: AppState) -> Router {
    use axum::routing::{delete, get, patch, post, put};

    Router::new()
        .route("/health", get(public::health))
        .route("/instances.json", get(public::get_instances))
        .route("/news.json", get(public::get_news))
        .route("/{game_dir}/manifest.json", get(public::get_manifest))
        .route("/files/{game_dir}/{filename}", get(public::get_file))
        .route("/extra/{id}/{*path}", get(extra_files::get_extra_file))
        .route("/updates/latest.json", get(updates::latest_json))
        .route("/updates/{platform}/{filename}", get(updates::get_update_file))
        .route("/admin", get(public::serve_admin))
        .route("/admin/{*path}", get(public::serve_admin))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/change-password", post(auth::change_password))
        .route(
            "/api/admin/instances",
            get(instances::list).post(instances::create),
        )
        .route("/api/admin/instances/order", put(instances::reorder))
        .route(
            "/api/admin/instances/{id}",
            put(instances::update).delete(instances::delete),
        )
        .route("/api/admin/news", get(news::list).post(news::upsert))
        .route("/api/admin/news/order", put(news::reorder))
        .route("/api/admin/news/{id}", delete(news::delete))
        .route(
            "/api/admin/instances/{id}/manifest",
            get(manifest::get_manifest).put(manifest::put_manifest),
        )
        .route(
            "/api/admin/instances/{id}/manifest/entry",
            patch(manifest::patch_manifest_entry),
        )
        .route(
            "/api/admin/instances/{id}/upload",
            post(manifest::upload).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/api/admin/instances/{id}/files/{filename}",
            delete(manifest::delete_file),
        )
        .route("/api/admin/instances/{id}/files", get(manifest::list_files))
        .route(
            "/api/admin/instances/{id}/extra-files/tree",
            get(extra_files::tree),
        )
        .route(
            "/api/admin/instances/{id}/extra-files/mkdir",
            post(extra_files::mkdir),
        )
        .route(
            "/api/admin/instances/{id}/extra-files/upload",
            post(extra_files::upload).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/api/admin/instances/{id}/extra-files/{*path}",
            delete(extra_files::delete_extra_file),
        )
        .route(
            "/api/admin/instances/{id}/scan",
            get(extra_files::scan),
        )
        .route(
            "/api/admin/instances/{id}/integrate",
            post(extra_files::integrate),
        )
        .route(
            "/api/admin/instances/{id}/rehash",
            post(extra_files::rehash),
        )
        .route("/api/admin/dashboard", get(dashboard::get_dashboard))
        .route(
            "/api/admin/updates",
            get(updates::get_release).put(updates::put_release_meta),
        )
        .route(
            "/api/admin/updates/{platform}/upload",
            post(updates::upload_platform).layer(DefaultBodyLimit::disable()),
        )
        .route("/api/admin/updates/{platform}", delete(updates::delete_platform))
        .route("/api/admin/trigger-build", post(updates::trigger_build))
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
                .allow_headers([
                    axum::http::header::CONTENT_TYPE,
                    axum::http::header::AUTHORIZATION,
                ]),
        )
}
