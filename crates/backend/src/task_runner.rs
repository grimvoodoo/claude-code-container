use std::process::Stdio;
use std::sync::Arc;

use anyhow::Result;
use shared::{TaskEvent, TaskStatus};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::Command;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::db;
use crate::git::build_clone_url;
use crate::state::{AppState, TaskLiveState};

/// Emit an event to the broadcast channel and persist it to the database.
async fn emit(state: &AppState, live: &TaskLiveState, task_id: Uuid, event: TaskEvent) {
    let json = match serde_json::to_string(&event) {
        Ok(j) => j,
        Err(e) => {
            error!("failed to serialize event: {e}");
            return;
        }
    };

    // Broadcast to any connected WebSocket (ignore if no receivers)
    let _ = live.event_tx.send(json);

    // Persist to DB (log but don't abort on failure)
    if let Err(e) = db::insert_event(&state.db, task_id, &event).await {
        error!(task_id = %task_id, "failed to persist event: {e}");
    }
}

/// Helper: run a child process and stream its output as events.
async fn run_process(
    state: &AppState,
    live: &TaskLiveState,
    task_id: Uuid,
    cmd: &str,
    args: &[&str],
    cwd: &str,
    env_pairs: &[(&str, String)],
) -> Result<std::process::ExitStatus> {
    let mut command = Command::new(cmd);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for (k, v) in env_pairs {
        command.env(k, v);
    }

    let mut child = command.spawn()?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    let mut stdout_reader = BufReader::new(stdout);
    let mut stderr_reader = BufReader::new(stderr);
    let mut buf = vec![0u8; 4096];
    let mut err_buf = vec![0u8; 4096];

    loop {
        tokio::select! {
            n = stdout_reader.read(&mut buf) => {
                match n {
                    Ok(0) => break,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                        emit(state, live, task_id, TaskEvent::Output { text }).await;
                    }
                    Err(e) => { error!("stdout read error: {e}"); break; }
                }
            }
            n = stderr_reader.read(&mut err_buf) => {
                match n {
                    Ok(0) => {}
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&err_buf[..n]).into_owned();
                        emit(state, live, task_id, TaskEvent::Stderr { text }).await;
                    }
                    Err(e) => { error!("stderr read error: {e}"); break; }
                }
            }
        }
    }

    Ok(child.wait().await?)
}

/// Spawn git to clone a repository, streaming output to the task.
async fn clone_repo(
    state: &AppState,
    live: &TaskLiveState,
    task_id: Uuid,
    repo: &str,
    branch: Option<&str>,
    token: &str,
    work_dir: &str,
) -> Result<()> {
    let clone_url = build_clone_url(repo, token)?;

    let branch_label = branch
        .map(|b| format!(" ({b})"))
        .unwrap_or_default();
    emit(
        state,
        live,
        task_id,
        TaskEvent::System {
            text: format!("Cloning {repo}{branch_label}…\r\n"),
        },
    )
    .await;

    let mut args = vec!["clone", "--depth", "1"];
    if let Some(b) = branch {
        args.extend_from_slice(&["--branch", b]);
    }
    // Clone into "." (the work_dir itself, already created)
    let clone_url_ref: &str = &clone_url;
    args.extend_from_slice(&[clone_url_ref, "."]);

    let status = run_process(state, live, task_id, "git", &args, work_dir, &[]).await?;
    if !status.success() {
        anyhow::bail!("git clone exited with {status}");
    }

    emit(
        state,
        live,
        task_id,
        TaskEvent::System {
            text: "Clone complete.\r\n".to_string(),
        },
    )
    .await;

    Ok(())
}

/// Configure git identity inside the workspace so Claude Code can commit.
async fn configure_git_identity(work_dir: &str) {
    let _ = Command::new("git")
        .args(["config", "user.email", "claude@container"])
        .current_dir(work_dir)
        .output()
        .await;

    let _ = Command::new("git")
        .args(["config", "user.name", "Claude Code"])
        .current_dir(work_dir)
        .output()
        .await;
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Run a task from scratch (first invocation or restart from user input).
///
/// `prompt`  — the instruction to pass to claude.
/// `is_restart` — when true, skip repo clone (workspace already populated).
pub async fn run_task(
    state: AppState,
    task_id: Uuid,
    prompt: String,
    repo: Option<String>,
    branch: Option<String>,
    is_restart: bool,
) {
    let workspace_base = std::env::var("WORKSPACE_DIR").unwrap_or_else(|_| "/workspace".into());
    let work_dir = format!("{workspace_base}/{task_id}");

    // Create workspace directory
    if let Err(e) = tokio::fs::create_dir_all(&work_dir).await {
        error!(task_id = %task_id, "failed to create workspace: {e}");
        return;
    }

    // Persist work_dir
    if let Err(e) = db::update_task_work_dir(&state.db, task_id, &work_dir).await {
        error!(task_id = %task_id, "failed to update work_dir: {e}");
    }

    // Get or create live state entry
    let live = state
        .live
        .entry(task_id)
        .or_insert_with(|| Arc::new(TaskLiveState::new()))
        .clone();

    // Update status → running
    if let Err(e) = db::update_task_status(&state.db, task_id, &TaskStatus::Running).await {
        error!(task_id = %task_id, "failed to update status: {e}");
    }
    emit(
        &state,
        &live,
        task_id,
        TaskEvent::Status {
            status: TaskStatus::Running,
            exit_code: None,
            signal: None,
        },
    )
    .await;

    // Clone repo on first run only
    if !is_restart {
        if let Some(ref repo_str) = repo {
            let mut clone_error: Option<String> = None;

            let github_token = std::env::var("GITHUB_TOKEN").unwrap_or_default();
            if github_token.is_empty() {
                clone_error =
                    Some("GITHUB_TOKEN is not set; cannot clone repository".to_string());
            } else if let Err(e) = clone_repo(
                &state,
                &live,
                task_id,
                repo_str,
                branch.as_deref(),
                &github_token,
                &work_dir,
            )
            .await
            {
                clone_error = Some(e.to_string());
            }

            if let Some(err) = clone_error {
                emit(
                    &state,
                    &live,
                    task_id,
                    TaskEvent::Stderr {
                        text: format!("Clone failed: {err}\r\n"),
                    },
                )
                .await;
                emit(
                    &state,
                    &live,
                    task_id,
                    TaskEvent::System {
                        text: "Handing off to Claude to decide how to proceed…\r\n".to_string(),
                    },
                )
                .await;
                // Prepend error context so Claude can respond or ask for help
                let prompt = format!(
                    "The repository clone failed with the following error:\n{err}\n\nOriginal task:\n{prompt}"
                );
                return run_claude(&state, &live, task_id, &prompt, &work_dir).await;
            }
        }
    }

    configure_git_identity(&work_dir).await;
    run_claude(&state, &live, task_id, &prompt, &work_dir).await;
}

async fn run_claude(
    state: &AppState,
    live: &TaskLiveState,
    task_id: Uuid,
    prompt: &str,
    work_dir: &str,
) {
    emit(
        state,
        live,
        task_id,
        TaskEvent::System {
            text: "Starting Claude Code…\r\n─────────────────────────────────────────\r\n"
                .to_string(),
        },
    )
    .await;

    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    let github_token = std::env::var("GITHUB_TOKEN").unwrap_or_default();

    let mut env_pairs: Vec<(&str, String)> = vec![
        ("HOME", home),
        ("NO_COLOR", "0".into()),
        ("FORCE_COLOR", "1".into()),
        ("GITHUB_TOKEN", github_token),
    ];

    if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
        env_pairs.push(("ANTHROPIC_API_KEY", api_key));
    }

    let args = ["--print", "--dangerously-skip-permissions", prompt];

    // Spawn claude, checking for cancellation
    let cancel = live.cancel.clone();

    let mut command = Command::new("claude");
    command
        .args(args)
        .current_dir(work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for (k, v) in &env_pairs {
        command.env(k, v);
    }

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            error!(task_id = %task_id, "failed to spawn claude: {e}");
            emit(
                state,
                live,
                task_id,
                TaskEvent::Stderr {
                    text: format!("Failed to start claude: {e}\r\n"),
                },
            )
            .await;
            finish_task(state, live, task_id, false, None, None).await;
            return;
        }
    };

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    let mut stdout_reader = BufReader::new(stdout);
    let mut stderr_reader = BufReader::new(stderr);
    let mut out_buf = vec![0u8; 4096];
    let mut err_buf = vec![0u8; 4096];
    let mut out_done = false;
    let mut err_done = false;

    loop {
        if out_done && err_done {
            break;
        }

        tokio::select! {
            _ = cancel.cancelled() => {
                info!(task_id = %task_id, "task cancelled");
                let _ = child.kill().await;
                return; // Caller will start a new run
            }
            n = stdout_reader.read(&mut out_buf), if !out_done => {
                match n {
                    Ok(0) => out_done = true,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&out_buf[..n]).into_owned();
                        emit(state, live, task_id, TaskEvent::Output { text }).await;
                    }
                    Err(e) => {
                        warn!("stdout read error: {e}");
                        out_done = true;
                    }
                }
            }
            n = stderr_reader.read(&mut err_buf), if !err_done => {
                match n {
                    Ok(0) => err_done = true,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&err_buf[..n]).into_owned();
                        emit(state, live, task_id, TaskEvent::Stderr { text }).await;
                    }
                    Err(e) => {
                        warn!("stderr read error: {e}");
                        err_done = true;
                    }
                }
            }
        }
    }

    let exit_status = match child.wait().await {
        Ok(s) => s,
        Err(e) => {
            error!(task_id = %task_id, "wait error: {e}");
            finish_task(state, live, task_id, false, None, None).await;
            return;
        }
    };

    let code = exit_status.code();
    let succeeded = code == Some(0);

    finish_task(state, live, task_id, succeeded, code, None).await;
}

async fn finish_task(
    state: &AppState,
    live: &TaskLiveState,
    task_id: Uuid,
    succeeded: bool,
    exit_code: Option<i32>,
    signal: Option<String>,
) {
    let status = if succeeded {
        TaskStatus::Completed
    } else {
        TaskStatus::Failed
    };

    let exit_label = exit_code
        .map(|c| c.to_string())
        .or_else(|| signal.clone())
        .unwrap_or_else(|| "?".into());

    if let Err(e) = db::update_task_status(&state.db, task_id, &status).await {
        error!(task_id = %task_id, "failed to persist final status: {e}");
    }

    emit(
        state,
        live,
        task_id,
        TaskEvent::Status {
            status: status.clone(),
            exit_code,
            signal,
        },
    )
    .await;

    emit(
        state,
        live,
        task_id,
        TaskEvent::System {
            text: format!(
                "\r\n─────────────────────────────────────────\r\nTask {} (exit {exit_label}).\r\n",
                status
            ),
        },
    )
    .await;
}
