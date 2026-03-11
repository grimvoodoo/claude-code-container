use dioxus::prelude::*;
use shared::{TaskStatus, TaskSummary};
use uuid::Uuid;

use crate::api;
use crate::components::{modal::NewTaskModal, sidebar::Sidebar, terminal::Terminal, toolbar::Toolbar};

const STYLES: &str = r#"
*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

:root {
  --bg: #0d1117;
  --surface: #161b22;
  --border: #30363d;
  --accent: #58a6ff;
  --accent-dim: #1f4e8c;
  --text: #e6edf3;
  --text-muted: #8b949e;
  --green: #3fb950;
  --red: #f85149;
  --yellow: #d29922;
  --radius: 6px;
}

body { background: var(--bg); color: var(--text); font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; height: 100vh; display: flex; flex-direction: column; }

header { padding: 12px 20px; background: var(--surface); border-bottom: 1px solid var(--border); display: flex; align-items: center; gap: 12px; flex-shrink: 0; }
header h1 { font-size: 15px; font-weight: 600; letter-spacing: .3px; }

.layout { flex: 1; display: flex; overflow: hidden; }

.sidebar { width: 280px; flex-shrink: 0; background: var(--surface); border-right: 1px solid var(--border); display: flex; flex-direction: column; overflow: hidden; }
.sidebar-header { padding: 12px; border-bottom: 1px solid var(--border); }
.sidebar-header button { width: 100%; padding: 8px 12px; background: var(--accent); color: #fff; border: none; border-radius: var(--radius); font-size: 13px; font-weight: 600; cursor: pointer; }
.sidebar-header button:hover { background: #79b8ff; }

.task-list { flex: 1; overflow-y: auto; }
.task-item { padding: 10px 14px; cursor: pointer; border-bottom: 1px solid var(--border); transition: background .12s; }
.task-item:hover { background: rgba(255,255,255,.04); }
.task-item.active { background: var(--accent-dim); }
.task-prompt { font-size: 13px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.task-meta { font-size: 11px; color: var(--text-muted); margin-top: 3px; display: flex; gap: 6px; align-items: center; }
.badge { display: inline-block; padding: 1px 6px; border-radius: 10px; font-size: 10px; font-weight: 600; text-transform: uppercase; }
.badge-running   { background: #1a3a5c; color: var(--accent); }
.badge-pending   { background: #2d2a1a; color: var(--yellow); }
.badge-completed { background: #1a3a25; color: var(--green); }
.badge-failed    { background: #3a1a1a; color: var(--red); }

.main { flex: 1; display: flex; flex-direction: column; overflow: hidden; }

.toolbar { padding: 8px 14px; border-bottom: 1px solid var(--border); display: flex; align-items: center; gap: 8px; flex-shrink: 0; font-size: 13px; color: var(--text-muted); }
.task-title { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cancel-btn { padding: 4px 10px; background: transparent; border: 1px solid var(--red); color: var(--red); border-radius: var(--radius); font-size: 12px; cursor: pointer; }
.cancel-btn:hover { background: rgba(248,81,73,.1); }

.terminal-wrap { flex: 1; overflow: hidden; padding: 6px; background: #0d1117; }
#xterm-container { height: 100%; }

.empty-state { flex: 1; display: flex; align-items: center; justify-content: center; color: var(--text-muted); font-size: 14px; flex-direction: column; gap: 8px; height: 100%; }

.input-bar { display: none; padding: 8px 10px; background: var(--surface); border-top: 1px solid var(--border); gap: 8px; align-items: center; flex-shrink: 0; }
.input-bar.visible { display: flex; }
.task-input { flex: 1; background: var(--bg); border: 1px solid var(--border); border-radius: var(--radius); color: var(--text); padding: 7px 10px; font-size: 13px; font-family: inherit; outline: none; }
.task-input:focus { border-color: var(--accent); }
.input-send { padding: 7px 14px; background: var(--accent); border: none; color: #fff; border-radius: var(--radius); font-size: 13px; font-weight: 600; cursor: pointer; white-space: nowrap; }
.input-send:hover { background: #79b8ff; }

.overlay { position: fixed; inset: 0; background: rgba(0,0,0,.6); display: flex; align-items: center; justify-content: center; z-index: 100; }
.overlay.hidden { display: none; }
.modal { background: var(--surface); border: 1px solid var(--border); border-radius: 10px; padding: 24px; width: 520px; max-width: 95vw; display: flex; flex-direction: column; gap: 16px; }
.modal h2 { font-size: 16px; }
label { font-size: 13px; color: var(--text-muted); display: block; margin-bottom: 4px; }
input, textarea { width: 100%; background: var(--bg); border: 1px solid var(--border); border-radius: var(--radius); color: var(--text); padding: 8px 10px; font-size: 13px; font-family: inherit; outline: none; }
input:focus, textarea:focus { border-color: var(--accent); }
textarea { resize: vertical; min-height: 100px; }
.modal-actions { display: flex; justify-content: flex-end; gap: 8px; }
.btn-cancel { padding: 8px 16px; background: transparent; border: 1px solid var(--border); color: var(--text); border-radius: var(--radius); cursor: pointer; font-size: 13px; }
.btn-submit { padding: 8px 16px; background: var(--accent); border: none; color: #fff; border-radius: var(--radius); cursor: pointer; font-size: 13px; font-weight: 600; }
.btn-submit:disabled { opacity: .5; cursor: not-allowed; }
"#;

#[component]
pub fn App() -> Element {
    let mut tasks: Signal<Vec<TaskSummary>> = use_signal(Vec::new);
    let mut active_task_id: Signal<Option<Uuid>> = use_signal(|| None);
    let mut active_status: Signal<Option<TaskStatus>> = use_signal(|| None);
    let mut show_modal: Signal<bool> = use_signal(|| false);

    // Fetch task list on mount and every 5 seconds
    use_coroutine(move |_rx: UnboundedReceiver<()>| async move {
        loop {
            if let Ok(list) = api::list_tasks().await {
                tasks.set(list);
            }
            gloo_timers::future::TimeoutFuture::new(5_000).await;
        }
    });

    let on_select_task = move |id: Uuid| {
        active_task_id.set(Some(id));
        // Look up initial status
        let status = tasks.read().iter().find(|t| t.id == id).map(|t| t.status.clone());
        active_status.set(status);
    };

    let on_status_change = move |status: TaskStatus| {
        active_status.set(Some(status.clone()));
        // Refresh sidebar badges
        spawn(async move {
            if let Ok(list) = api::list_tasks().await {
                tasks.set(list);
            }
        });
    };

    let on_cancel = move |_| {
        if let Some(id) = active_task_id() {
            spawn(async move {
                let _ = api::cancel_task(id).await;
            });
        }
    };

    let on_task_created = move |id: Uuid| {
        spawn(async move {
            if let Ok(list) = api::list_tasks().await {
                tasks.set(list);
            }
        });
        active_task_id.set(Some(id));
        active_status.set(Some(TaskStatus::Pending));
    };

    rsx! {
        style { {STYLES} }

        header {
            h1 { "Claude Code Server" }
        }

        div { class: "layout",
            Sidebar {
                tasks: tasks.read().clone(),
                active_task_id: active_task_id(),
                on_select: on_select_task,
                on_new_task: move |_| show_modal.set(true),
            }

            main { class: "main",
                Toolbar {
                    tasks: tasks.read().clone(),
                    active_task_id: active_task_id(),
                    active_status: active_status(),
                    on_cancel,
                }

                Terminal {
                    active_task_id: active_task_id(),
                    on_status_change,
                }
            }
        }

        if show_modal() {
            NewTaskModal {
                on_close: move |_| show_modal.set(false),
                on_created: on_task_created,
            }
        }
    }
}
