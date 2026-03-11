use dioxus::prelude::*;
use shared::{TaskStatus, TaskSummary};
use uuid::Uuid;

#[component]
pub fn Toolbar(
    tasks: Vec<TaskSummary>,
    active_task_id: Option<Uuid>,
    active_status: Option<TaskStatus>,
    on_cancel: EventHandler<()>,
) -> Element {
    let prompt = active_task_id
        .and_then(|id| tasks.iter().find(|t| t.id == id).map(|t| t.prompt.clone()))
        .unwrap_or_else(|| "Select or create a task".into());

    let is_running = active_status.as_ref() == Some(&TaskStatus::Running);

    rsx! {
        div { class: "toolbar",
            span { class: "task-title", "{prompt}" }
            if is_running {
                button {
                    class: "cancel-btn",
                    onclick: move |_| on_cancel.call(()),
                    "Cancel"
                }
            }
        }
    }
}
