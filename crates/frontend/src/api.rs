use anyhow::Result;
use gloo_net::http::Request;
use shared::{CreateTaskRequest, CreateTaskResponse, TaskSummary};
use uuid::Uuid;

const API_BASE: &str = "/api/tasks";

pub async fn list_tasks() -> Result<Vec<TaskSummary>> {
    let tasks = Request::get(API_BASE)
        .send()
        .await?
        .json::<Vec<TaskSummary>>()
        .await?;
    Ok(tasks)
}

pub async fn create_task(
    prompt: String,
    repo: Option<String>,
    branch: Option<String>,
) -> Result<Uuid> {
    let body = CreateTaskRequest { prompt, repo, branch };
    let resp = Request::post(API_BASE)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&body)?)?
        .send()
        .await?;

    if !resp.ok() {
        anyhow::bail!("server returned {}", resp.status());
    }

    let created = resp.json::<CreateTaskResponse>().await?;
    Ok(created.id)
}

pub async fn cancel_task(id: Uuid) -> Result<()> {
    Request::delete(&format!("{API_BASE}/{id}"))
        .send()
        .await?;
    Ok(())
}
