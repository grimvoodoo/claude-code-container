use anyhow::Result;
use chrono::{DateTime, Utc};
use shared::{TaskEvent, TaskStatus, TaskSummary};
use sqlx::PgPool;
use uuid::Uuid;

// ── Task row ─────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
pub struct TaskRow {
    pub id: Uuid,
    pub prompt: String,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub status: String,
    pub work_dir: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<TaskRow> for TaskSummary {
    fn from(r: TaskRow) -> Self {
        TaskSummary {
            id: r.id,
            prompt: r.prompt,
            repo: r.repo,
            status: r.status.parse().unwrap_or(TaskStatus::Pending),
            created_at: r.created_at,
        }
    }
}

// ── Event row ─────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct EventRow {
    event_type: String,
    text: Option<String>,
    status: Option<String>,
    exit_code: Option<i32>,
    signal: Option<String>,
}

impl From<EventRow> for Option<TaskEvent> {
    fn from(r: EventRow) -> Self {
        match r.event_type.as_str() {
            "output" => Some(TaskEvent::Output {
                text: r.text.unwrap_or_default(),
            }),
            "stderr" => Some(TaskEvent::Stderr {
                text: r.text.unwrap_or_default(),
            }),
            "system" => Some(TaskEvent::System {
                text: r.text.unwrap_or_default(),
            }),
            "status" => Some(TaskEvent::Status {
                status: r
                    .status
                    .unwrap_or_default()
                    .parse()
                    .unwrap_or(TaskStatus::Pending),
                exit_code: r.exit_code,
                signal: r.signal,
            }),
            _ => None,
        }
    }
}

// ── Queries ───────────────────────────────────────────────────────────────────

pub async fn list_tasks(db: &PgPool) -> Result<Vec<TaskSummary>> {
    let rows = sqlx::query_as::<_, TaskRow>(
        "SELECT id, prompt, repo, branch, status, work_dir, created_at
         FROM tasks
         ORDER BY created_at DESC",
    )
    .fetch_all(db)
    .await?;

    Ok(rows.into_iter().map(TaskSummary::from).collect())
}

pub async fn get_task(db: &PgPool, id: Uuid) -> Result<Option<TaskRow>> {
    let row = sqlx::query_as::<_, TaskRow>(
        "SELECT id, prompt, repo, branch, status, work_dir, created_at
         FROM tasks WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;

    Ok(row)
}

pub async fn create_task(
    db: &PgPool,
    id: Uuid,
    prompt: &str,
    repo: Option<&str>,
    branch: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO tasks (id, prompt, repo, branch, status)
         VALUES ($1, $2, $3, $4, 'pending')",
    )
    .bind(id)
    .bind(prompt)
    .bind(repo)
    .bind(branch)
    .execute(db)
    .await?;

    Ok(())
}

pub async fn update_task_status(db: &PgPool, id: Uuid, status: &TaskStatus) -> Result<()> {
    sqlx::query("UPDATE tasks SET status = $1 WHERE id = $2")
        .bind(status.to_string())
        .bind(id)
        .execute(db)
        .await?;

    Ok(())
}

pub async fn update_task_work_dir(db: &PgPool, id: Uuid, work_dir: &str) -> Result<()> {
    sqlx::query("UPDATE tasks SET work_dir = $1 WHERE id = $2")
        .bind(work_dir)
        .bind(id)
        .execute(db)
        .await?;

    Ok(())
}

pub async fn insert_event(db: &PgPool, task_id: Uuid, event: &TaskEvent) -> Result<()> {
    let (event_type, text, status, exit_code, signal): (
        &str,
        Option<&str>,
        Option<String>,
        Option<i32>,
        Option<&str>,
    ) = match event {
        TaskEvent::Output { text } => ("output", Some(text.as_str()), None, None, None),
        TaskEvent::Stderr { text } => ("stderr", Some(text.as_str()), None, None, None),
        TaskEvent::System { text } => ("system", Some(text.as_str()), None, None, None),
        TaskEvent::Status {
            status,
            exit_code,
            signal,
        } => (
            "status",
            None,
            Some(status.to_string()),
            *exit_code,
            signal.as_deref(),
        ),
        TaskEvent::InputError { error } => ("input_error", Some(error.as_str()), None, None, None),
    };

    sqlx::query(
        "INSERT INTO task_events (task_id, event_type, text, status, exit_code, signal)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(task_id)
    .bind(event_type)
    .bind(text)
    .bind(status)
    .bind(exit_code)
    .bind(signal)
    .execute(db)
    .await?;

    Ok(())
}

pub async fn get_task_events(db: &PgPool, task_id: Uuid) -> Result<Vec<TaskEvent>> {
    let rows = sqlx::query_as::<_, EventRow>(
        "SELECT event_type, text, status, exit_code, signal
         FROM task_events
         WHERE task_id = $1
         ORDER BY id ASC",
    )
    .bind(task_id)
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|r| Option::<TaskEvent>::from(r))
        .collect())
}
