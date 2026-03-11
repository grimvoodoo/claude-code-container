use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::json;
use shared::{CreateTaskRequest, CreateTaskResponse};
use uuid::Uuid;

use crate::{db, state::AppState, task_runner};

// ── Error type ────────────────────────────────────────────────────────────────

pub enum AppError {
    NotFound,
    BadRequest(String),
    Internal(anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, Json(json!({"error": "Not found"}))).into_response(),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, Json(json!({"error": msg}))).into_response(),
            AppError::Internal(e) => {
                tracing::error!("internal error: {e:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Internal server error"}))).into_response()
            }
        }
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError::Internal(e.into())
    }
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// GET /api/tasks — list all tasks (summary only, sorted newest first)
pub async fn list_tasks(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let tasks = db::list_tasks(&state.db).await?;
    Ok(Json(tasks))
}

/// GET /api/tasks/:id — get a single task with its full event history
pub async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let task = db::get_task(&state.db, id)
        .await?
        .ok_or(AppError::NotFound)?;

    let events = db::get_task_events(&state.db, id).await?;

    Ok(Json(json!({
        "id": task.id,
        "prompt": task.prompt,
        "repo": task.repo,
        "branch": task.branch,
        "status": task.status,
        "workDir": task.work_dir,
        "createdAt": task.created_at,
        "events": events,
    })))
}

/// POST /api/tasks — create and immediately start a new task
pub async fn create_task(
    State(state): State<AppState>,
    Json(body): Json<CreateTaskRequest>,
) -> Result<impl IntoResponse, AppError> {
    let prompt = body.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err(AppError::BadRequest("prompt is required".into()));
    }

    let id = Uuid::new_v4();
    let repo = body.repo.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let branch = body
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    db::create_task(&state.db, id, &prompt, repo, branch).await?;

    // Spawn task runner in background
    let state_clone = state.clone();
    let prompt_clone = prompt.clone();
    let repo_owned = repo.map(String::from);
    let branch_owned = branch.map(String::from);
    tokio::spawn(async move {
        task_runner::run_task(state_clone, id, prompt_clone, repo_owned, branch_owned, false)
            .await;
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(CreateTaskResponse { id }),
    ))
}

/// DELETE /api/tasks/:id — cancel a running task
pub async fn cancel_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    db::get_task(&state.db, id)
        .await?
        .ok_or(AppError::NotFound)?;

    // Signal cancellation if task is live
    if let Some(live) = state.live.get(&id) {
        live.cancel.cancel();
    }

    Ok(Json(json!({"ok": true})))
}
