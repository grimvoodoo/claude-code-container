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

  // Handle input from the browser.
  // Because Claude Code runs in --print (non-interactive) mode, stdin is not available.
  // Instead, we kill the current run and restart Claude in the same workspace with the
  // new instruction, so all previously written files are still present.
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
    if (!t) {
      ws.send(JSON.stringify({ type: 'input_error', taskId, error: 'Task not found' }));
      return;
    }
    if (!t.workDir) {
      ws.send(JSON.stringify({ type: 'input_error', taskId, error: 'Task workspace not ready yet' }));
      return;
    }

    // Kill the running process if there is one
    if (t.process) {
      t.process.kill('SIGTERM');
      t.process = null;
    }

    emit(t, { type: 'system', text: `\r\n─────────────────────────────────────────\r\nNew instruction received — restarting Claude in the same workspace…\r\n─────────────────────────────────────────\r\n` });

    // Update the prompt and re-run in the existing workspace (skipping clone/git setup)
    t.prompt = msg.text;
    runTask(t, t.workDir);
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
    workDir: null,
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

// Accepts any common repo reference and returns an authenticated HTTPS clone URL.
// Handled formats:
//   owner/repo
//   github.com/owner/repo
//   https://github.com/owner/repo
//   https://github.com/owner/repo/pull/123   (PR URLs — /pull/... is stripped)
//   git@github.com:owner/repo.git            (SSH)
//   github.com:owner/repo                    (SSH-style without git@)
function buildCloneUrl(raw, token) {
  raw = raw.trim();
  let owner, repo;

  if (raw.startsWith('git@')) {
    // git@github.com:owner/repo.git
    const m = raw.match(/git@[^:]+:([^/]+)\/([^/.]+)(\.git)?$/);
    if (m) { owner = m[1]; repo = m[2]; }
  } else if (!raw.startsWith('http') && raw.includes(':')) {
    // github.com:owner/repo  or  github.com:owner/repo.git
    const m = raw.match(/:([^/]+)\/([^/.]+)(\.git)?$/);
    if (m) { owner = m[1]; repo = m[2]; }
  } else {
    // https://github.com/owner/repo[/pull/123][.git]  or  owner/repo
    const path = raw
      .replace(/^https?:\/\//, '')   // strip protocol
      .replace(/^github\.com\//, '') // strip hostname
      .replace(/\.git$/, '');        // strip trailing .git
    const parts = path.split('/');
    // Take only the first two segments — ignores /pull/123, /tree/branch, etc.
    if (parts.length >= 2) { owner = parts[0]; repo = parts[1]; }
  }

  if (!owner || !repo) throw new Error(`Could not parse repository: "${raw}"`);

  return `https://x-access-token:${token}@github.com/${owner}/${repo}.git`;
}

async function runTask(task, workDir) {
  const isRestart = !!workDir;
  workDir = workDir || path.join(WORKSPACE_BASE, task.id);
  task.workDir = workDir;

  try {
    fs.mkdirSync(workDir, { recursive: true });
    task.status = 'running';
    emit(task, { type: 'status', status: 'running' });

    // Clone repo on first run only — skip on restarts triggered by user input
    if (task.repo && !isRestart) {
      let cloneError = null;

      if (!process.env.GITHUB_TOKEN) {
        cloneError = 'GITHUB_TOKEN is not set; cannot clone repository';
      } else {
        try {
          const repoUrl = buildCloneUrl(task.repo, process.env.GITHUB_TOKEN);
          emit(task, { type: 'system', text: `Cloning ${task.repo}${task.branch ? ` (${task.branch})` : ''}…\r\n` });
          const cloneArgs = ['clone', '--depth', '1'];
          if (task.branch) cloneArgs.push('--branch', task.branch);
          cloneArgs.push(repoUrl, '.');
          await spawnAsync('git', cloneArgs, workDir, task);
          emit(task, { type: 'system', text: 'Clone complete.\r\n' });
        } catch (err) {
          cloneError = err.message;
        }
      }

      if (cloneError) {
        emit(task, { type: 'stderr', text: `Clone failed: ${cloneError}\r\n` });
        emit(task, { type: 'system', text: 'Handing off to Claude to decide how to proceed…\r\n' });
        // Prepend the failure so Claude has full context and can respond or ask for help
        task.prompt = `The repository clone failed with the following error:\n${cloneError}\n\nOriginal task:\n${task.prompt}`;
      }
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
      stdio: ['ignore', 'pipe', 'pipe'],
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
