use dioxus::prelude::*;
use shared::TaskSummary;
use uuid::Uuid;

fn fmt_date(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.format("%b %-d, %H:%M").to_string()
}

#[component]
pub fn Sidebar(
    tasks: Vec<TaskSummary>,
    active_task_id: Option<Uuid>,
    on_select: EventHandler<Uuid>,
    on_new_task: EventHandler<()>,
) -> Element {
    rsx! {
        aside { class: "sidebar",
            div { class: "sidebar-header",
                button { onclick: move |_| on_new_task.call(()), "+ New Task" }
            }

            div { class: "task-list",
                if tasks.is_empty() {
                    div { style: "padding:16px;font-size:13px;color:var(--text-muted)", "No tasks yet." }
                } else {
                    for task in tasks.iter() {
                        TaskItem {
                            key: "{task.id}",
                            task: task.clone(),
                            active: active_task_id == Some(task.id),
                            on_select,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TaskItem(task: TaskSummary, active: bool, on_select: EventHandler<Uuid>) -> Element {
    let id = task.id;
    let status_class = format!("badge badge-{}", task.status);
    let status_label = task.status.to_string();
    let active_class = if active { "task-item active" } else { "task-item" };
    let date_str = fmt_date(&task.created_at);

    rsx! {
        div {
            class: "{active_class}",
            onclick: move |_| on_select.call(id),

            div { class: "task-prompt", "{task.prompt}" }
            div { class: "task-meta",
                span { class: "{status_class}", "{status_label}" }
                span { "{date_str}" }
                if let Some(ref repo) = task.repo {
                    span { "📁 {repo}" }
                }
            }
        }
    }
}
