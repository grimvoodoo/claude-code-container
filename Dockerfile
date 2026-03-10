# ── Stage 1: install Node deps ────────────────────────────────────────────────
FROM node:20-slim AS deps

WORKDIR /app
COPY package.json ./
RUN npm install --omit=dev

# ── Stage 2: final image ──────────────────────────────────────────────────────
FROM node:20-slim

# System tools needed by Claude Code
RUN apt-get update && apt-get install -y --no-install-recommends \
    git \
    curl \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Install Claude Code CLI globally
RUN npm install -g @anthropic-ai/claude-code

# node:20-slim already ships a 'node' user at UID 1000 — use it
USER node

WORKDIR /app

# Copy deps and source
COPY --from=deps --chown=node:node /app/node_modules ./node_modules
COPY --chown=node:node package.json ./
COPY --chown=node:node src/ ./src/

# Workspace lives on a volume
VOLUME ["/workspace"]

ENV PORT=3000
ENV WORKSPACE_DIR=/workspace
ENV NODE_ENV=production

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl -fs http://localhost:3000/api/tasks || exit 1

CMD ["node", "src/server.js"]
