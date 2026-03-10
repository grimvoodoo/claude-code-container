/**
 * Smoke tests for claude-code-server.
 *
 * These tests run against a live container with no Claude credentials present,
 * so tasks are expected to fail quickly. The assertions verify that:
 *   - HTTP endpoints respond correctly
 *   - Tasks can be created via the API
 *   - WebSocket streaming delivers output and a terminal status event
 *
 * Run against a local container:
 *   node --test tests/smoke.mjs
 *
 * Run against a different host:
 *   BASE_URL=http://192.168.1.10:30123 node --test tests/smoke.mjs
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { WebSocket } from 'ws';

const BASE = process.env.BASE_URL || 'http://localhost:3000';
const WS_BASE = BASE.replace(/^http/, 'ws');
const TASK_TIMEOUT_MS = 60_000;

// ── Helpers ───────────────────────────────────────────────────────────────────

async function get(path) {
  const res = await fetch(`${BASE}${path}`);
  return { status: res.status, body: await res.json() };
}

async function post(path, data) {
  const res = await fetch(`${BASE}${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  });
  return { status: res.status, body: await res.json() };
}

function subscribeTask(id) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(`${WS_BASE}?taskId=${id}`);
    const events = [];
    const timer = setTimeout(() => {
      ws.close();
      reject(new Error(`Timed out after ${TASK_TIMEOUT_MS}ms waiting for task to finish`));
    }, TASK_TIMEOUT_MS);

    ws.on('message', (raw) => {
      const msg = JSON.parse(raw.toString());
      events.push(msg);
      if (msg.type === 'status' && (msg.status === 'completed' || msg.status === 'failed')) {
        clearTimeout(timer);
        ws.close();
        resolve(events);
      }
    });

    ws.on('error', (err) => {
      clearTimeout(timer);
      reject(err);
    });
  });
}

// ── Tests ─────────────────────────────────────────────────────────────────────

test('GET /api/tasks returns 200 with an array', async () => {
  const { status, body } = await get('/api/tasks');
  assert.equal(status, 200, 'expected HTTP 200');
  assert.ok(Array.isArray(body), 'expected response to be an array');
});

test('POST /api/tasks with missing prompt returns 400', async () => {
  const { status } = await post('/api/tasks', {});
  assert.equal(status, 400, 'expected HTTP 400 for missing prompt');
});

test('POST /api/tasks creates a task and returns an id', async () => {
  const { status, body } = await post('/api/tasks', { prompt: 'echo hello' });
  assert.equal(status, 202, 'expected HTTP 202');
  assert.ok(typeof body.id === 'string' && body.id.length > 0, 'expected a task id');
});

test('GET /api/tasks/:id returns the task', async () => {
  const { body: created } = await post('/api/tasks', { prompt: 'echo hello' });
  const { status, body: task } = await get(`/api/tasks/${created.id}`);
  assert.equal(status, 200, 'expected HTTP 200');
  assert.equal(task.id, created.id, 'expected matching task id');
  assert.ok(task.status, 'expected a status field');
});

test('GET /api/tasks/:id returns 404 for unknown id', async () => {
  const { status } = await get('/api/tasks/00000000-0000-0000-0000-000000000000');
  assert.equal(status, 404, 'expected HTTP 404');
});

test('WebSocket streams output and reaches a terminal status (no credentials)', async () => {
  // Create a simple task. Without Claude credentials the process will exit
  // quickly with an auth error — that is the expected behaviour here.
  const { body: created } = await post('/api/tasks', { prompt: 'echo hello' });
  assert.ok(created.id, 'task created');

  const events = await subscribeTask(created.id);

  const statusEvents = events.filter((e) => e.type === 'status');
  const terminalEvent = statusEvents.find(
    (e) => e.status === 'completed' || e.status === 'failed',
  );
  assert.ok(terminalEvent, 'task should reach a terminal status (completed or failed)');

  const outputEvents = events.filter(
    (e) => e.type === 'output' || e.type === 'stderr' || e.type === 'system',
  );
  assert.ok(outputEvents.length > 0, 'should have received at least one output event over WebSocket');
});

test('Completed tasks appear in GET /api/tasks list', async () => {
  const { body: created } = await post('/api/tasks', { prompt: 'echo hello' });
  await subscribeTask(created.id);

  const { body: list } = await get('/api/tasks');
  const found = list.find((t) => t.id === created.id);
  assert.ok(found, 'task should appear in the task list after completion');
  assert.ok(
    found.status === 'completed' || found.status === 'failed',
    `expected terminal status, got: ${found.status}`,
  );
});
