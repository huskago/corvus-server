use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

use crate::{error::AppError, models::NewsItem, routes::AuthUser, state::AppState, storage};

pub async fn list(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<Vec<NewsItem>>, AppError> {
    storage::read_news(&state.data_dir)
        .await
        .map(Json)
        .map_err(AppError::Storage)
}

pub async fn upsert(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(item): Json<NewsItem>,
) -> Result<Json<NewsItem>, AppError> {
    let _guard = state.write_lock.lock().await;
    let mut items = storage::read_news(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;

    match items.iter().position(|n| n.id == item.id) {
        Some(pos) => items[pos] = item.clone(),
        None => items.insert(0, item.clone()),
    }

    storage::write_news(&state.data_dir, &items)
        .await
        .map_err(AppError::Storage)?;

    Ok(Json(item))
}

pub async fn delete(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let _guard = state.write_lock.lock().await;
    let mut items = storage::read_news(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;

    let pos = items
        .iter()
        .position(|n| n.id == id)
        .ok_or_else(|| AppError::NotFound(format!("news '{id}' not found")))?;

    items.remove(pos);
    storage::write_news(&state.data_dir, &items)
        .await
        .map_err(AppError::Storage)?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn reorder(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(order): Json<Vec<String>>,
) -> Result<StatusCode, AppError> {
    let _guard = state.write_lock.lock().await;
    let items = storage::read_news(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;

    let reordered: Vec<NewsItem> = order
        .iter()
        .filter_map(|id| items.iter().find(|n| &n.id == id).cloned())
        .collect();

    storage::write_news(&state.data_dir, &reordered)
        .await
        .map_err(AppError::Storage)?;

    Ok(StatusCode::NO_CONTENT)
}
