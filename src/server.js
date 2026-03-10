import express from 'express';
import { WebSocketServer } from 'ws';
import { createServer } from 'http';
import { spawn } from 'child_process';
import { randomUUID } from 'crypto';
import path from 'path';
import { fileURLToPath } from 'url';
import fs from 'fs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const app = express();
const server = createServer(app);
const wss = new WebSocketServer({ server });

app.use(express.json());
app.use(express.static(path.join(__dirname, 'public')));

const WORKSPACE_BASE = process.env.WORKSPACE_DIR || '/workspace';
const PORT = parseInt(process.env.PORT || '3000', 10);

// In-memory task store. Replace with a DB for persistence across restarts.
const tasks = new Map();

// One WebSocket connection per task (latest subscriber wins)
const taskSockets = new Map();

// ── WebSocket connection handler ──────────────────────────────────────────────
wss.on('connection', (ws, req) => {
  const params = new URL(req.url, 'http://localhost').searchParams;
  const taskId = params.get('taskId');
  if (!taskId) { ws.close(); return; }

  taskSockets.set(taskId, ws);

  // Replay history to the new subscriber
  const task = tasks.get(taskId);
  if (task) {
    for (const event of task.events) {
      ws.send(JSON.stringify(event));
    }
  }

  // Forward input from the browser to the running process's stdin
  ws.on('message', (raw) => {
    let msg;
    try {
      msg = JSON.parse(raw.toString());
    } catch (err) {
      console.error(`[ws] invalid JSON from client (taskId=${taskId}):`, err.message);
      ws.send(JSON.stringify({ type: 'input_error', taskId, error: 'Invalid JSON: ' + err.message }));
      return;
    }

    if (msg.type !== 'input') return;

    const t = tasks.get(taskId);
    if (!t?.process?.stdin?.writable) {
      ws.send(JSON.stringify({ type: 'input_error', taskId, error: 'No running process to send input to' }));
      return;
    }

    t.process.stdin.write(msg.text + '\n', (err) => {
      if (err) {
        console.error(`[ws] stdin.write failed (taskId=${taskId}):`, err.message);
        ws.send(JSON.stringify({ type: 'input_error', taskId, error: 'Failed to write to process: ' + err.message }));
      }
    });
  });

  ws.on('close', () => {
    if (taskSockets.get(taskId) === ws) taskSockets.delete(taskId);
  });
});

// ── REST API ──────────────────────────────────────────────────────────────────

// List all tasks (summary)
app.get('/api/tasks', (_req, res) => {
  const list = Array.from(tasks.values()).map(({ id, prompt, repo, status, createdAt }) => ({
    id, prompt, repo, status, createdAt,
  }));
  res.json(list.sort((a, b) => b.createdAt.localeCompare(a.createdAt)));
});

// Get a single task (full, including events)
app.get('/api/tasks/:id', (req, res) => {
  const task = tasks.get(req.params.id);
  if (!task) return res.status(404).json({ error: 'Not found' });
  res.json(task);
});

// Cancel a running task
app.delete('/api/tasks/:id', (req, res) => {
  const task = tasks.get(req.params.id);
  if (!task) return res.status(404).json({ error: 'Not found' });
  if (task.process && task.status === 'running') {
    task.process.kill('SIGTERM');
  }
  res.json({ ok: true });
});

// Create a new task
app.post('/api/tasks', (req, res) => {
  const { prompt, repo, branch } = req.body ?? {};
  if (!prompt || typeof prompt !== 'string' || !prompt.trim()) {
    return res.status(400).json({ error: 'prompt is required' });
  }

  const id = randomUUID();
  const task = {
    id,
    prompt: prompt.trim(),
    repo: repo?.trim() || null,
    branch: branch?.trim() || null,
    status: 'pending',
    events: [],
    process: null,
    createdAt: new Date().toISOString(),
  };
  tasks.set(id, task);
  res.status(202).json({ id });

  // Run asynchronously so the response is returned immediately
  setImmediate(() => runTask(task));
});

// ── Task runner ───────────────────────────────────────────────────────────────

function emit(task, event) {
  task.events.push(event);
  const ws = taskSockets.get(task.id);
  if (ws && ws.readyState === 1 /* OPEN */) {
    ws.send(JSON.stringify(event));
  }
}

async function runTask(task) {
  const workDir = path.join(WORKSPACE_BASE, task.id);

  try {
    fs.mkdirSync(workDir, { recursive: true });
    task.status = 'running';
    emit(task, { type: 'status', status: 'running' });

    // Clone repo if provided
    if (task.repo) {
      const token = process.env.GITHUB_TOKEN;
      if (!token) throw new Error('GITHUB_TOKEN is not set; cannot clone repository');

      const repoUrl = task.repo.startsWith('http')
        ? task.repo.replace('https://', `https://x-access-token:${token}@`)
        : `https://x-access-token:${token}@github.com/${task.repo}.git`;

      emit(task, { type: 'system', text: `Cloning ${task.repo}${task.branch ? ` (${task.branch})` : ''}…\r\n` });

      const cloneArgs = ['clone', '--depth', '1'];
      if (task.branch) cloneArgs.push('--branch', task.branch);
      cloneArgs.push(repoUrl, '.');

      await spawnAsync('git', cloneArgs, workDir, task);
      emit(task, { type: 'system', text: 'Clone complete.\r\n' });
    }

    // Configure git identity inside the workspace so Claude Code can commit
    await spawnAsync('git', ['config', 'user.email', 'claude@container'], workDir, null).catch(() => {});
    await spawnAsync('git', ['config', 'user.name', 'Claude Code'], workDir, null).catch(() => {});

    emit(task, { type: 'system', text: `Starting Claude Code…\r\n─────────────────────────────────────────\r\n` });

    const claudeEnv = {
      ...process.env,
      HOME: process.env.HOME || '/home/node',
      // ANTHROPIC_API_KEY is optional — subscription auth uses ~/.claude/ credentials
      ...(process.env.ANTHROPIC_API_KEY && { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY }),
      GITHUB_TOKEN: process.env.GITHUB_TOKEN,
      NO_COLOR: '0',
      FORCE_COLOR: '1',
    };

    const claudeArgs = [
      '--print',
      '--dangerously-skip-permissions',
      task.prompt,
    ];

    const proc = spawn('claude', claudeArgs, {
      cwd: workDir,
      env: claudeEnv,
      stdio: ['pipe', 'pipe', 'pipe'],
    });

    task.process = proc;

    proc.stdout.on('data', (chunk) => {
      emit(task, { type: 'output', text: chunk.toString() });
    });

    proc.stderr.on('data', (chunk) => {
      emit(task, { type: 'stderr', text: chunk.toString() });
    });

    await new Promise((resolve) => {
      proc.on('close', (code, signal) => {
        task.process = null;
        const succeeded = code === 0;
        task.status = succeeded ? 'completed' : 'failed';
        emit(task, {
          type: 'status',
          status: task.status,
          exitCode: code,
          signal,
        });
        emit(task, {
          type: 'system',
          text: `\r\n─────────────────────────────────────────\r\nTask ${task.status} (exit ${code ?? signal}).\r\n`,
        });
        resolve();
      });
    });

  } catch (err) {
    task.status = 'failed';
    emit(task, { type: 'stderr', text: `\r\nError: ${err.message}\r\n` });
    emit(task, { type: 'status', status: 'failed' });
  }
}

function spawnAsync(cmd, args, cwd, task) {
  return new Promise((resolve, reject) => {
    const proc = spawn(cmd, args, { cwd, stdio: ['ignore', 'pipe', 'pipe'] });
    proc.stdout.on('data', (d) => task && emit(task, { type: 'output', text: d.toString() }));
    proc.stderr.on('data', (d) => task && emit(task, { type: 'stderr', text: d.toString() }));
    proc.on('close', (code) => (code === 0 ? resolve() : reject(new Error(`${cmd} exited with ${code}`))));
  });
}

// ── Start ─────────────────────────────────────────────────────────────────────

// Auth: either ANTHROPIC_API_KEY (API key) or ~/.claude/ credentials (subscription login)
if (!process.env.ANTHROPIC_API_KEY) {
  console.log('[info] No ANTHROPIC_API_KEY — will use ~/.claude/ subscription credentials');
}

server.listen(PORT, () => {
  console.log(`claude-code-server listening on http://0.0.0.0:${PORT}`);
});
