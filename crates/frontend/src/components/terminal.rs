use dioxus::prelude::*;
use shared::TaskStatus;
use uuid::Uuid;

/// The terminal component owns the xterm.js session.
/// It opens a WebSocket connection (via JavaScript) whenever `active_task_id` changes,
/// and exposes an input bar for sending follow-up instructions to Claude.
#[component]
pub fn Terminal(
    active_task_id: Option<Uuid>,
    on_status_change: EventHandler<TaskStatus>,
) -> Element {
    let mut task_status: Signal<Option<TaskStatus>> = use_signal(|| None);
    let mut input_text: Signal<String> = use_signal(String::new);
    // Track which task we last opened a session for, to avoid reopening on every render.
    let mut session_task_id: Signal<Option<Uuid>> = use_signal(|| None);

    // When active task changes, open a new xterm.js + WebSocket session via JS.
    use_effect(move || {
        if session_task_id() == active_task_id {
            return;
        }
        session_task_id.set(active_task_id);

        match active_task_id {
            None => {
                document::eval("window.termApp.closeSession()");
            }
            Some(task_id) => {
                let script = format!(
                    r#"window.termApp.openSession("{task_id}", function(status) {{
                        window.__termStatus = status;
                    }});"#
                );
                document::eval(&script);
            }
        }
    });

    // Poll for status changes posted by the JS WebSocket handler.
    use_coroutine(move |_rx: UnboundedReceiver<()>| async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(500).await;

            let mut eval = document::eval("return window.__termStatus || null;");
            if let Ok(val) = eval.recv::<serde_json::Value>().await {
                if let Some(status_str) = val.as_str() {
                    let status: Option<TaskStatus> = match status_str {
                        "running" => Some(TaskStatus::Running),
                        "completed" => Some(TaskStatus::Completed),
                        "failed" => Some(TaskStatus::Failed),
                        "pending" => Some(TaskStatus::Pending),
                        _ => None,
                    };
                    if let Some(s) = status {
                        if task_status.read().as_ref() != Some(&s) {
                            task_status.set(Some(s.clone()));
                            on_status_change.call(s);
                            document::eval("window.__termStatus = null;");
                        }
                    }
                }
            }
        }
    });

    let is_running = task_status.read().as_ref() == Some(&TaskStatus::Running);

    let input_bar_class = if is_running {
        "input-bar visible"
    } else {
        "input-bar"
    };

    rsx! {
        div { class: "terminal-wrap",
            if active_task_id.is_none() {
                div { class: "empty-state",
                    div { "No task selected" }
                    div { style: "font-size:12px",
                        "Create a new task or select one from the sidebar"
                    }
                }
            } else {
                div { id: "xterm-container", style: "height:100%;" }
            }
        }

        div { class: "{input_bar_class}",
            input {
                class: "task-input",
                r#type: "text",
                placeholder: "Send a new instruction — Claude will restart in the same workspace…",
                autocomplete: "off",
                value: "{input_text}",
                oninput: move |e| input_text.set(e.value()),
                onkeydown: move |e| {
                    if e.key() == Key::Enter {
                        let text = input_text.read().trim().to_string();
                        if text.is_empty() {
                            return;
                        }
                        input_text.set(String::new());
                        let script = format!(
                            "window.termApp.sendInput({});",
                            serde_json::to_string(&text).unwrap_or_default()
                        );
                        spawn(async move {
                            document::eval(&script);
                        });
                    }
                },
            }
            button {
                class: "input-send",
                onclick: move |_| {
                    let text = input_text.read().trim().to_string();
                    if text.is_empty() {
                        return;
                    }
                    input_text.set(String::new());
                    let script = format!(
                        "window.termApp.sendInput({});",
                        serde_json::to_string(&text).unwrap_or_default()
                    );
                    spawn(async move {
                        document::eval(&script);
                    });
                },
                "Send"
            }
        }
    }
}
