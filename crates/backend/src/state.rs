use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Per-task live state (exists only while the server is running).
/// Historical events are persisted in the DB and replayed on WebSocket connect.
pub struct TaskLiveState {
    /// Broadcast channel — every event is sent as a JSON string.
    pub event_tx: broadcast::Sender<String>,
    /// Cancel token — cancelled when the task should stop (user cancel or restart).
    pub cancel: CancellationToken,
}

impl TaskLiveState {
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            event_tx,
            cancel: CancellationToken::new(),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    /// Live state keyed by task UUID. Entries are created when a task starts
    /// and remain until removed (tasks are never truly removed from memory
    /// in this impl, but the channel clears automatically).
    pub live: Arc<DashMap<Uuid, Arc<TaskLiveState>>>,
}

impl AppState {
    pub fn new(db: sqlx::PgPool) -> Self {
        Self {
            db,
            live: Arc::new(DashMap::new()),
        }
    }
}
