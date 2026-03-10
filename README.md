# Claude Code Server

Run [Claude Code](https://docs.anthropic.com/en/docs/claude-code) as a persistent web service. Submit prompts through a browser UI, point it at a GitHub repo, and let Claude work autonomously — no laptop required.

## What it does

- Accepts a prompt (and optionally a GitHub repo + branch) through a web UI
- Clones the repo into an isolated workspace directory
- Runs `claude --print` headlessly so Claude can read files, write code, and run commands
- Streams all output back to your browser in real time via WebSocket

```
Browser  ──HTTP/WS──►  Express (Node.js)  ──spawn──►  claude --print
```

---

## Prerequisites

- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) installed locally (`npm install -g @anthropic-ai/claude-code`)
- A Claude subscription **or** an Anthropic API key
- Docker (for local use) or a Kubernetes cluster (for deployment)

---

## Quick start with Docker

**1. Log in to Claude on your host machine** (only needed once)

```bash
claude login
```

This creates `~/.claude/.credentials.json`, which the container will use for auth.

**2. Configure environment**

```bash
cp .env.example .env
```

Open `.env` and add your `GITHUB_TOKEN` if you want Claude to clone private repos. Everything else is optional.

**3. Start the server**

```bash
docker compose up --build
```

Open [http://localhost:3000](http://localhost:3000).

> **API key instead of a subscription?** Uncomment `ANTHROPIC_API_KEY` in `.env` and `docker-compose.yml`. The credentials file mount is then not needed.

---

## Authentication

| Method | How to set up |
|--------|--------------|
| **Claude subscription** (default) | Run `claude login` on your host. `docker-compose.yml` mounts `~/.claude/.credentials.json` read-only into the container. |
| **API key** | Set `ANTHROPIC_API_KEY` as an environment variable. Remove the credentials file mount from `docker-compose.yml`. |

---

## Using the UI

1. Click **+ New Task**
2. Enter a prompt — describe what you want Claude to do
3. Optionally provide a GitHub repo (`owner/repo`) and branch
4. Click **Run** — the task starts immediately and output streams to the terminal

| Feature | Detail |
|---------|--------|
| Live output | Full ANSI colour terminal (xterm.js) |
| Cancel | Sends SIGTERM to the running Claude process |
| Sidebar | All tasks listed with status badges, auto-refreshes every 5 s |
| Reconnect | Replays full output history if you navigate away and come back |

---

## Deploying to Kubernetes

The repo ships with Kubernetes manifests in `k8s/` (maintained separately). The GitHub Actions workflow automatically builds and publishes the image to GitHub Container Registry on every push to `main`.

### Automated image builds

Merging a pull request into `main` triggers `.github/workflows/build.yml`, which:

1. Reads the latest git tag to determine the current version
2. Increments the patch version (`0.0.1` → `0.0.2` → etc.), starting at `0.0.1` on the first release
3. Creates and pushes the new git tag
4. Builds the image with Docker Buildx and pushes to `ghcr.io/<your-github-username>/claude-container` with two tags:
   - `0.0.1` (or whatever the new version is) — pinned, immutable tag
   - `latest` — always points to the most recent release

No repository secrets need to be configured — the workflow uses the built-in `GITHUB_TOKEN`.

> Pushing directly to `main` without a PR will **not** trigger a release.

### Making the image publicly pullable

If your cluster needs to pull without registry credentials:

> GitHub → your package → **Package settings** → **Change visibility** → Public

### Applying the manifests

```bash
# Create namespace and storage
kubectl apply -f k8s/namespace.yaml
kubectl apply -f k8s/pvc.yaml

# Create the credentials secret (see your k8s setup for details)
kubectl apply -f k8s/secret.yaml

# Deploy
kubectl apply -f k8s/deployment.yaml
kubectl apply -f k8s/service.yaml
```

The service is a `NodePort` on port **30123** — accessible at `http://<any-node-ip>:30123`.

If you use MetalLB or a cloud provider load balancer, change `type: NodePort` to `type: LoadBalancer` in `k8s/service.yaml`.

### Updating after a new image is pushed

```bash
kubectl rollout restart deployment/claude-code-server -n claude-code
```

---

## Environment variables

| Variable | Required | Description |
|----------|----------|-------------|
| `ANTHROPIC_API_KEY` | No* | API key auth. Not needed if using subscription credentials. |
| `GITHUB_TOKEN` | No | Personal access token for cloning private repos. |
| `WORKSPACE_DIR` | No | Where task workspaces are created. Default: `/workspace` |
| `PORT` | No | Port the server listens on. Default: `3000` |

---

## Security notes

- `--dangerously-skip-permissions` is passed to Claude Code so it can operate without interactive prompts. **Only run this on a trusted network** — there is no authentication on the web UI.
- Tasks are held in memory. The server must run as a single replica unless you add an external store (Redis, Postgres, etc.).
- Workspace directories persist on the PVC and are not automatically cleaned up. Remove old directories manually if disk space becomes a concern.
# claude-code-container
# claude-code-container
