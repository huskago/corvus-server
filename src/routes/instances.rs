use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

use crate::{error::AppError, models::InstanceInfo, routes::AuthUser, state::AppState, storage};

pub async fn list(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<Vec<InstanceInfo>>, AppError> {
    storage::read_instances(&state.data_dir)
        .await
        .map(Json)
        .map_err(AppError::Storage)
}

pub async fn create(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(instance): Json<InstanceInfo>,
) -> Result<(StatusCode, Json<InstanceInfo>), AppError> {
    let _guard = state.write_lock.lock().await;
    let mut items = storage::read_instances(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;

    if items.iter().any(|i| i.game_dir_name == instance.game_dir_name) {
        return Err(AppError::Conflict(format!(
            "instance '{}' already exists",
            instance.game_dir_name
        )));
    }

    items.push(instance.clone());
    storage::write_instances(&state.data_dir, &items)
        .await
        .map_err(AppError::Storage)?;

    Ok((StatusCode::CREATED, Json(instance)))
}

pub async fn update(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Json(instance): Json<InstanceInfo>,
) -> Result<Json<InstanceInfo>, AppError> {
    let _guard = state.write_lock.lock().await;
    let mut items = storage::read_instances(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;

    let pos = items
        .iter()
        .position(|i| i.game_dir_name == id)
        .ok_or_else(|| AppError::NotFound(format!("instance '{id}' not found")))?;

    items[pos] = instance.clone();
    storage::write_instances(&state.data_dir, &items)
        .await
        .map_err(AppError::Storage)?;

    Ok(Json(instance))
}

pub async fn delete(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let _guard = state.write_lock.lock().await;
    let mut items = storage::read_instances(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;

    let pos = items
        .iter()
        .position(|i| i.game_dir_name == id)
        .ok_or_else(|| AppError::NotFound(format!("instance '{id}' not found")))?;

    items.remove(pos);
    storage::write_instances(&state.data_dir, &items)
        .await
        .map_err(AppError::Storage)?;

    let instance_dir = state.instance_dir(&id);
    if instance_dir.exists() {
        tokio::fs::remove_dir_all(&instance_dir)
            .await
            .map_err(|e| AppError::Storage(format!("failed to delete instance dir: {e}")))?;
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn reorder(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(order): Json<Vec<String>>,
) -> Result<StatusCode, AppError> {
    let _guard = state.write_lock.lock().await;
    let items = storage::read_instances(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;

    if order.len() != items.len() || !items.iter().all(|i| order.contains(&i.game_dir_name)) {
        return Err(AppError::BadRequest(
            "order must contain exactly all existing instance IDs".to_string())
        );
    }

    let reordered: Vec<InstanceInfo> = order
        .iter()
        .filter_map(|id| items.iter().find(|i| &i.game_dir_name == id).cloned())
        .collect();

    storage::write_instances(&state.data_dir, &reordered)
        .await
        .map_err(AppError::Storage)?;

    Ok(StatusCode::NO_CONTENT)
}
