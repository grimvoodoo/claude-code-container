# ── Stage 1: Build frontend (Dioxus WASM) ────────────────────────────────────
FROM docker.io/rust:1.82-slim AS frontend-builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Add WASM target
RUN rustup target add wasm32-unknown-unknown

# Install Dioxus CLI
RUN cargo install dioxus-cli --locked

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/shared/ ./crates/shared/
COPY crates/frontend/ ./crates/frontend/

# Build the Dioxus WASM frontend
WORKDIR /app/crates/frontend
RUN dx build --release

# ── Stage 2: Build backend (Axum server) ─────────────────────────────────────
FROM docker.io/rust:1.82-slim AS backend-builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/shared/ ./crates/shared/
COPY crates/backend/ ./crates/backend/
# Dummy frontend crate so the workspace resolves
COPY crates/frontend/Cargo.toml ./crates/frontend/Cargo.toml
RUN mkdir -p crates/frontend/src && echo "fn main(){}" > crates/frontend/src/main.rs

COPY migrations/ ./migrations/

RUN cargo build --release --package backend

# ── Stage 3: Final image ──────────────────────────────────────────────────────
FROM docker.io/debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl ca-certificates git nodejs npm \
    && rm -rf /var/lib/apt/lists/*

# Install Claude Code CLI
RUN npm install -g @anthropic-ai/claude-code

# Copy the server binary
COPY --from=backend-builder /app/target/release/claude-container /usr/local/bin/claude-container

# Copy the compiled WASM frontend
COPY --from=frontend-builder /app/crates/frontend/dist/ /app/dist/

WORKDIR /app

VOLUME ["/workspace"]

ENV PORT=3000
ENV WORKSPACE_DIR=/workspace
ENV STATIC_DIR=/app/dist

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
  CMD curl -fs http://localhost:3000/api/tasks || exit 1

CMD ["claude-container"]
