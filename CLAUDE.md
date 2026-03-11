# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Project Is

A headless web service that runs Claude Code as a persistent service. Users submit prompts via a browser UI (optionally pointing to a GitHub repo), and Claude works autonomously in an ephemeral workspace. Real-time output streams back to the browser over WebSocket.

**Flow:** Browser (Dioxus WASM) → HTTP/WebSocket → Axum server → spawns `claude --print` → streams output back → persisted in PostgreSQL

## Crate Structure

```
crates/
  shared/     — types shared between server and frontend (Task, TaskEvent, etc.)
  backend/    — Axum HTTP/WebSocket server + task runner (binary: claude-container)
  frontend/   — Dioxus web app compiled to WASM
migrations/   — SQLx migrations (run automatically on server startup)
```

## Commands

```bash
# Build & run everything (Docker — recommended)
docker compose up --build

# Backend only (requires a running Postgres and DATABASE_URL in .env)
cargo run --package backend

# Frontend dev server (hot-reload, requires dx CLI)
cd crates/frontend && dx serve

# Build frontend for production
cd crates/frontend && dx build --release
# Output goes to crates/frontend/dist/

# Install dx CLI (one-time)
cargo install dioxus-cli --locked

# Run backend tests (unit tests only — no live container needed)
cargo test --package backend

# Run all workspace tests
cargo test
```

## Architecture

### Backend (`crates/backend/`)

| File | Role |
|------|------|
| `main.rs` | Sets up Axum router, runs SQLx migrations, starts server |
| `state.rs` | `AppState` (db pool + `DashMap` of per-task live state) |
| `db.rs` | All SQLx queries — tasks + task_events tables |
| `git.rs` | Parses GitHub URLs into authenticated HTTPS clone URLs |
| `task_runner.rs` | Spawns `claude --print`, streams stdout/stderr, emits events |
| `routes/tasks.rs` | REST handlers: list/get/create/cancel |
| `routes/ws.rs` | WebSocket: replays DB history on connect, subscribes to broadcast |

**Event flow:** `task_runner` emits `TaskEvent` → serialised to JSON → `broadcast::Sender<String>` (live WebSocket) + `INSERT` into `task_events` (persistence). New WebSocket connections replay from DB then subscribe to the broadcast channel.

**Process restarts (user input):** WebSocket handler cancels the current `CancellationToken`, inserts a new `TaskLiveState` with a fresh token, then re-spawns `run_task` with `is_restart = true` (skips git clone).

### Frontend (`crates/frontend/`)

Dioxus 0.6 web app compiled to WASM. Components:

| Component | Role |
|-----------|------|
| `app.rs` | Root component — app-level state (task list, active task, modal toggle) |
| `sidebar.rs` | Task list with status badges; polls `/api/tasks` every 5 s |
| `toolbar.rs` | Shows active prompt + Cancel button when running |
| `terminal.rs` | Manages xterm.js session + input bar via `document::eval()` |
| `modal.rs` | New task form (prompt, repo, branch) |

**xterm.js integration:** The terminal component calls JavaScript via `document::eval()` to drive `window.termApp` (defined in `index.html`). `termApp.openSession(taskId)` opens the WebSocket and writes events to xterm.js. Status changes are polled back into Dioxus state via `window.__termStatus`.

### Database

Two tables: `tasks` and `task_events`. Migrations live in `migrations/` and are embedded into the binary via `sqlx::migrate!` — they run automatically on startup.

## Environment Variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `DATABASE_URL` | PostgreSQL connection string | (required) |
| `ANTHROPIC_API_KEY` | API key auth | uses `~/.claude/` if unset |
| `GITHUB_TOKEN` | Clone private repos | (optional) |
| `WORKSPACE_DIR` | Where task workspaces are created | `/workspace` |
| `PORT` | Server port | `3000` |
| `STATIC_DIR` | Path to compiled WASM frontend | `./dist` |

## Key Design Decisions

- **PostgreSQL replaces in-memory storage** — tasks and events survive server restarts. Multi-replica deployments work without extra coordination.
- **xterm.js managed via JS interop** — the terminal UI uses JavaScript (`window.termApp` in `index.html`) called from Dioxus via `document::eval()`. This gives full terminal emulation with minimal Rust complexity.
- **Process restarts on user input** — since `claude` runs `--print` (non-interactive), user follow-up prompts kill the running process and re-invoke Claude in the same workspace directory.
- **Clone errors are non-fatal** — when git clone fails, the error is prepended to the prompt so Claude can diagnose and respond.
