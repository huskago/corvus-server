use axum::{extract::State, Json};

use crate::{error::AppError, models::DashboardStats, routes::AuthUser, state::AppState, storage};

pub async fn get_dashboard(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<DashboardStats>, AppError> {
    let instances = storage::read_instances(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;
    let news = storage::read_news(&state.data_dir)
        .await
        .map_err(AppError::Storage)?;

    let mut total_files: usize = 0;
    let mut total_size_bytes: u64 = 0;

    for instance in &instances {
        let files_dir = state.files_dir(&instance.game_dir_name);
        if let Ok(mut entries) = tokio::fs::read_dir(&files_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(meta) = entry.metadata().await {
                    total_files += 1;
                    total_size_bytes += meta.len();
                }
            }
        }
    }

    Ok(Json(DashboardStats {
        instance_count: instances.len(),
        news_count: news.len(),
        total_files,
        total_size_bytes,
        uptime_secs: state.started_at.elapsed().as_secs(),
    }))
}
