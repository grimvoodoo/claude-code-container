use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use shared::{TaskEvent, WsClientMessage};
use tokio::sync::broadcast;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{db, state::{AppState, TaskLiveState}, task_runner};

#[derive(Deserialize)]
pub struct WsQuery {
    #[serde(rename = "taskId")]
    task_id: Uuid,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<WsQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, params.task_id, state))
}

async fn handle_socket(socket: WebSocket, task_id: Uuid, state: AppState) {
    let (mut sender, mut receiver) = socket.split();

    // Replay historical events from DB
    match db::get_task_events(&state.db, task_id).await {
        Ok(events) => {
            for event in events {
                if let Ok(json) = serde_json::to_string(&event) {
                    if sender.send(Message::Text(json.into())).await.is_err() {
                        return; // Client disconnected during replay
                    }
                }
            }
        }
        Err(e) => {
            error!(task_id = %task_id, "failed to load events for replay: {e}");
        }
    }

    // Subscribe to live events broadcast
    let rx = state
        .live
        .get(&task_id)
        .map(|live| live.event_tx.subscribe());

    // Forward live events to the WebSocket
    let send_task = {
        let mut rx_opt = rx;
        async move {
            if let Some(ref mut rx) = rx_opt {
                loop {
                    match rx.recv().await {
                        Ok(json) => {
                            if sender.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!(task_id = %task_id, "WebSocket lagged {n} messages");
                        }
                    }
                }
            }
            // Keep sender alive so we can drop it cleanly
            let _ = sender.close().await;
        }
    };

    // Handle incoming messages from the browser
    let recv_task = {
        let state = state.clone();
        async move {
            while let Some(Ok(msg)) = receiver.next().await {
                let text = match msg {
                    Message::Text(t) => t,
                    Message::Close(_) => break,
                    _ => continue,
                };

                let client_msg: WsClientMessage = match serde_json::from_str(&text) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(task_id = %task_id, "invalid WS message: {e}");
                        continue;
                    }
                };

                let WsClientMessage::Input { text: prompt } = client_msg;

                // Fetch task metadata to check it exists and get repo/branch
                let task = match db::get_task(&state.db, task_id).await {
                    Ok(Some(t)) => t,
                    Ok(None) => {
                        warn!(task_id = %task_id, "task not found for input");
                        continue;
                    }
                    Err(e) => {
                        error!(task_id = %task_id, "db error on input: {e}");
                        continue;
                    }
                };

                // Cancel the currently running process (if any)
                if let Some(live) = state.live.get(&task_id) {
                    live.cancel.cancel();
                }

                // Brief yield so the cancelled task has time to die
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                // Emit a divider system message
                {
                    let divider = TaskEvent::System {
                        text: "\r\n─────────────────────────────────────────\r\nNew instruction received — restarting Claude in the same workspace…\r\n─────────────────────────────────────────\r\n".to_string(),
                    };

                    let new_live = Arc::new(TaskLiveState::new());
                    state.live.insert(task_id, new_live.clone());

                    if let Ok(json) = serde_json::to_string(&divider) {
                        let _ = new_live.event_tx.send(json.clone());
                        if let Err(e) = db::insert_event(&state.db, task_id, &divider).await {
                            error!(task_id = %task_id, "failed to persist divider: {e}");
                        }
                    }
                }

                // Restart claude in the same workspace (is_restart = true skips clone)
                let state_clone = state.clone();
                tokio::spawn(async move {
                    task_runner::run_task(
                        state_clone,
                        task_id,
                        prompt,
                        task.repo,
                        task.branch,
                        true, // is_restart
                    )
                    .await;
                });
            }
        }
    };

    tokio::select! {
        _ = send_task => {}
        _ = recv_task => {}
    }

    info!(task_id = %task_id, "WebSocket connection closed");
}
