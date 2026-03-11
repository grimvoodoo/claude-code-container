use dioxus::prelude::*;
use uuid::Uuid;

use crate::api;

#[component]
pub fn NewTaskModal(
    on_close: EventHandler<()>,
    on_created: EventHandler<Uuid>,
) -> Element {
    let mut prompt: Signal<String> = use_signal(String::new);
    let mut repo: Signal<String> = use_signal(String::new);
    let mut branch: Signal<String> = use_signal(String::new);
    let mut submitting: Signal<bool> = use_signal(|| false);
    let mut error: Signal<Option<String>> = use_signal(|| None);

    let submit = move |_| {
        let p = prompt.read().trim().to_string();
        if p.is_empty() {
            return;
        }
        let r = repo.read().trim().to_string();
        let b = branch.read().trim().to_string();

        submitting.set(true);
        error.set(None);

        spawn(async move {
            let repo_opt = if r.is_empty() { None } else { Some(r) };
            let branch_opt = if b.is_empty() { None } else { Some(b) };

            match api::create_task(p, repo_opt, branch_opt).await {
                Ok(id) => {
                    on_created.call(id);
                    on_close.call(());
                }
                Err(e) => {
                    error.set(Some(e.to_string()));
                    submitting.set(false);
                }
            }
        });
    };

    let close = move |_| on_close.call(());

    rsx! {
        div {
            class: "overlay",
            onclick: close,

            div {
                class: "modal",
                // Stop clicks inside the modal from closing it
                onclick: move |e| e.stop_propagation(),

                h2 { "New Task" }

                div {
                    label { r#for: "f-prompt", "Prompt *" }
                    textarea {
                        id: "f-prompt",
                        placeholder: "Describe what you want Claude Code to do…",
                        value: "{prompt}",
                        oninput: move |e| prompt.set(e.value()),
                    }
                }

                div {
                    label { r#for: "f-repo", "GitHub repo (optional)" }
                    input {
                        id: "f-repo",
                        r#type: "text",
                        placeholder: "owner/repo  or  https://github.com/owner/repo",
                        value: "{repo}",
                        oninput: move |e| repo.set(e.value()),
                    }
                }

                div {
                    label { r#for: "f-branch", "Branch (optional)" }
                    input {
                        id: "f-branch",
                        r#type: "text",
                        placeholder: "main",
                        value: "{branch}",
                        oninput: move |e| branch.set(e.value()),
                    }
                }

                if let Some(ref err) = error() {
                    div { style: "color: var(--red); font-size: 13px;", "{err}" }
                }

                div { class: "modal-actions",
                    button { class: "btn-cancel", onclick: close, "Cancel" }
                    button {
                        class: "btn-submit",
                        disabled: submitting(),
                        onclick: submit,
                        if submitting() { "Running…" } else { "Run" }
                    }
                }
            }
        }
    }
}
