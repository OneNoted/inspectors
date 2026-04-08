import { createServer, IncomingMessage, Server, ServerResponse } from 'node:http';
import { mkdir, stat } from 'node:fs/promises';
import { createReadStream, existsSync } from 'node:fs';
import { dirname, extname, join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { randomUUID } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { firefox, type BrowserContext, type Page } from 'playwright-core';
import type { ActionReceipt, ActionRequest, JsonObject, RuntimeCapabilities, SessionRecord, TaskRecord } from './types.js';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, '../../..');
const uiRoot = join(repoRoot, 'apps', 'web-ui', 'public');
const artifactRoot = join(repoRoot, 'artifacts');
const defaultGuestRuntimeUrl = process.env.GUEST_RUNTIME_URL ?? 'http://127.0.0.1:4001';
const playwrightEnabled = process.env.ACU_ENABLE_PLAYWRIGHT === '1';

type TaskStatus = TaskRecord['status'];

interface BrowserState {
  context: BrowserContext;
  page: Page;
  browserName: 'firefox';
}

interface BrowserSnapshotCache {
  lastUrl?: string;
  lastHtml?: string;
  fetchedAt?: string;
}

interface ControlPlaneState {
  guestRuntimeUrl: string;
  tasks: Map<string, TaskRecord>;
  actionHistory: Map<string, JsonObject[]>;
  browserStates: Map<string, BrowserState>;
  browserSnapshots: Map<string, BrowserSnapshotCache>;
}

export function toGuestAction(action: ActionRequest): JsonObject {
  const taskId = 'taskId' in action ? action.taskId : undefined;
  switch (action.kind) {
    case 'mouse_move':
      return { kind: action.kind, x: action.x, y: action.y, task_id: taskId ?? null };
    case 'mouse_click':
      return { kind: action.kind, button: action.button ?? 'left', x: action.x ?? null, y: action.y ?? null, task_id: taskId ?? null };
    case 'mouse_drag':
      return { kind: action.kind, start_x: action.start_x, start_y: action.start_y, end_x: action.end_x, end_y: action.end_y, task_id: taskId ?? null };
    case 'key_press':
      return { kind: action.kind, key: action.key, task_id: taskId ?? null };
    case 'type_text':
      return { kind: action.kind, text: action.text, task_id: taskId ?? null };
    case 'hotkey':
      return { kind: action.kind, keys: action.keys, task_id: taskId ?? null };
    case 'scroll':
      return { kind: action.kind, delta_x: action.delta_x, delta_y: action.delta_y, task_id: taskId ?? null };
    case 'open_app':
      return { kind: action.kind, name: action.name, task_id: taskId ?? null };
    case 'focus_window':
      return { kind: action.kind, window_id: action.window_id, task_id: taskId ?? null };
    case 'resize_window':
      return { kind: action.kind, window_id: action.window_id, bounds: action.bounds, task_id: taskId ?? null };
    case 'run_command':
      return { kind: action.kind, command: action.command, cwd: action.cwd ?? null, env: action.env ?? null, task_id: taskId ?? null };
    case 'read_file':
      return { kind: action.kind, path: action.path, task_id: taskId ?? null };
    case 'write_file':
      return { kind: action.kind, path: action.path, contents: action.contents, task_id: taskId ?? null };
    case 'browser_open':
      return { kind: action.kind, url: action.url, task_id: taskId ?? null };
    case 'browser_get_dom':
      return { kind: action.kind, task_id: taskId ?? null };
    case 'browser_click':
      return { kind: action.kind, selector: action.selector ?? null, x: action.x ?? null, y: action.y ?? null, button: action.button ?? 'left', task_id: taskId ?? null };
    case 'browser_type':
      return { kind: action.kind, selector: action.selector ?? null, text: action.text, task_id: taskId ?? null };
    case 'browser_screenshot':
      return { kind: action.kind, task_id: taskId ?? null };
  }
}

async function readJson(req: IncomingMessage): Promise<Record<string, unknown>> {
  const chunks: Buffer[] = [];
  for await (const chunk of req) {
    chunks.push(Buffer.from(chunk));
  }
  if (chunks.length === 0) return {};
  return JSON.parse(Buffer.concat(chunks).toString('utf8')) as Record<string, unknown>;
}

function json(res: ServerResponse, status: number, payload: unknown): void {
  res.statusCode = status;
  res.setHeader('content-type', 'application/json; charset=utf-8');
  res.end(JSON.stringify(payload, null, 2));
}

function mapTaskStatus(verb: string): TaskStatus {
  if (verb === 'pause') return 'paused';
  if (verb === 'resume') return 'running';
  if (verb === 'terminate') return 'terminated';
  return 'completed';
}

async function fetchBrowserSnapshot(url: string): Promise<BrowserSnapshotCache | null> {
  try {
    if (url.startsWith('data:text/html,')) {
      const html = decodeURIComponent(url.slice('data:text/html,'.length));
      return { lastUrl: url, lastHtml: html, fetchedAt: new Date().toISOString() };
    }
    if (url.startsWith('http://') || url.startsWith('https://')) {
      const response = await fetch(url);
      const html = await response.text();
      return { lastUrl: url, lastHtml: html, fetchedAt: new Date().toISOString() };
    }
  } catch {
    return null;
  }
  return null;
}

async function guestJson<T>(state: ControlPlaneState, path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${state.guestRuntimeUrl}${path}`, {
    headers: { 'content-type': 'application/json', ...(init?.headers ?? {}) },
    ...init,
  });
  const text = await response.text();
  const payload = text ? JSON.parse(text) : null;
  if (!response.ok) {
    throw new Error(typeof payload?.error?.message === 'string' ? payload.error.message : `${path} failed with ${response.status}`);
  }
  return payload as T;
}

async function getGuestSession(state: ControlPlaneState, sessionId: string): Promise<SessionRecord> {
  const payload = await guestJson<{ session: SessionRecord }>(state, `/api/sessions/${sessionId}`);
  return payload.session;
}

async function ensureBrowser(state: ControlPlaneState, sessionId: string): Promise<BrowserState> {
  if (!playwrightEnabled) {
    throw new Error('playwright browser adapter is disabled in this environment');
  }
  const existing = state.browserStates.get(sessionId);
  if (existing) return existing;
  const session = await getGuestSession(state, sessionId);
  if (!session.display) {
    throw new Error('session does not expose DISPLAY');
  }
  const executablePath = process.env.FIREFOX_EXECUTABLE ?? '/usr/bin/firefox';
  if (!existsSync(executablePath)) {
    throw new Error(`firefox executable not found at ${executablePath}`);
  }
  const userDataDir = join(tmpdir(), `acu-browser-${sessionId}`);
  await mkdir(userDataDir, { recursive: true });
  const context = await firefox.launchPersistentContext(userDataDir, {
    executablePath,
    headless: false,
    env: { ...process.env, DISPLAY: session.display },
  });
  const page = context.pages()[0] ?? await context.newPage();
  const browserState: BrowserState = { context, page, browserName: 'firefox' };
  state.browserStates.set(sessionId, browserState);
  return browserState;
}

async function closeBrowser(state: ControlPlaneState, sessionId: string): Promise<void> {
  const browser = state.browserStates.get(sessionId);
  if (!browser) return;
  await browser.context.close();
  state.browserStates.delete(sessionId);
}

function pushHistory(state: ControlPlaneState, sessionId: string, entry: JsonObject): void {
  const list = state.actionHistory.get(sessionId) ?? [];
  list.unshift(entry);
  state.actionHistory.set(sessionId, list.slice(0, 250));
}

function attachReceiptToTask(state: ControlPlaneState, taskId: string | undefined, receipt: ActionReceipt): void {
  if (!taskId) return;
  const task = state.tasks.get(taskId);
  if (!task) return;
  task.lastReceipt = receipt;
  task.updatedAt = new Date().toISOString();
  if (task.status === 'pending') task.status = 'running';
  state.tasks.set(taskId, task);
}

type BrowserAction = Extract<ActionRequest, { kind: 'browser_open' | 'browser_get_dom' | 'browser_click' | 'browser_type' | 'browser_screenshot' }>;

function unsupportedBrowserReceipt(action: BrowserAction, message: string): ActionReceipt {
  return {
    status: 'error',
    receipt_id: randomUUID(),
    action_type: action.kind,
    started_at: new Date().toISOString(),
    completed_at: new Date().toISOString(),
    result: {},
    artifacts: [],
    error: { code: 'browser_dom_unavailable', message, retryable: false, category: 'browser', details: {}, artifact_refs: [] },
  };
}

async function handleBrowserAction(state: ControlPlaneState, sessionId: string, action: BrowserAction): Promise<ActionReceipt> {
  if (!playwrightEnabled) {
    if (action.kind === 'browser_open') {
      const receipt = await guestJson<ActionReceipt>(state, `/api/sessions/${sessionId}/actions`, { method: 'POST', body: JSON.stringify(toGuestAction(action)) });
      const snapshot = await fetchBrowserSnapshot(action.url);
      if (snapshot) state.browserSnapshots.set(sessionId, snapshot);
      pushHistory(state, sessionId, { action: action as unknown as JsonObject, receipt: receipt as unknown as JsonObject, source: 'browser-open-fallback' });
      attachReceiptToTask(state, action.taskId, receipt);
      return receipt;
    }
    if (action.kind === 'browser_click' && typeof action.x === 'number' && typeof action.y === 'number') {
      const receipt = await guestJson<ActionReceipt>(state, `/api/sessions/${sessionId}/actions`, { method: 'POST', body: JSON.stringify(toGuestAction({ kind: 'mouse_click', button: action.button, x: action.x, y: action.y, taskId: action.taskId })) });
      pushHistory(state, sessionId, { action: action as unknown as JsonObject, receipt: receipt as unknown as JsonObject, source: 'browser-coordinate-fallback' });
      attachReceiptToTask(state, action.taskId, receipt);
      return receipt;
    }
    if (action.kind === 'browser_type' && !action.selector) {
      const receipt = await guestJson<ActionReceipt>(state, `/api/sessions/${sessionId}/actions`, { method: 'POST', body: JSON.stringify(toGuestAction({ kind: 'type_text', text: action.text, taskId: action.taskId })) });
      pushHistory(state, sessionId, { action: action as unknown as JsonObject, receipt: receipt as unknown as JsonObject, source: 'browser-coordinate-fallback' });
      attachReceiptToTask(state, action.taskId, receipt);
      return receipt;
    }
    if (action.kind === 'browser_screenshot') {
      const observation = await guestJson<Record<string, unknown>>(state, `/api/sessions/${sessionId}/observation`);
      const screenshot = (observation.screenshot ?? {}) as Record<string, unknown>;
      const receipt: ActionReceipt = {
        status: 'ok',
        receipt_id: randomUUID(),
        action_type: action.kind,
        started_at: new Date().toISOString(),
        completed_at: new Date().toISOString(),
        result: { path: screenshot.artifact_path ?? null },
        artifacts: screenshot.artifact_path ? [{ kind: 'browser_screenshot', path: String(screenshot.artifact_path), mime_type: 'image/png' }] : [],
        error: null,
      };
      pushHistory(state, sessionId, { action: action as unknown as JsonObject, receipt: receipt as unknown as JsonObject, source: 'browser-screenshot-fallback' });
      attachReceiptToTask(state, action.taskId, receipt);
      return receipt;
    }
    if (action.kind === 'browser_get_dom') {
      const snapshot = state.browserSnapshots.get(sessionId);
      if (snapshot?.lastHtml) {
        const receipt: ActionReceipt = {
          status: 'ok',
          receipt_id: randomUUID(),
          action_type: action.kind,
          started_at: new Date().toISOString(),
          completed_at: new Date().toISOString(),
          result: { current_url: snapshot.lastUrl ?? null, title: null, dom_html: snapshot.lastHtml, console_logs: [], network_events: [] },
          artifacts: [],
          error: null,
        };
        pushHistory(state, sessionId, { action: action as unknown as JsonObject, receipt: receipt as unknown as JsonObject, source: 'browser-dom-fetch-fallback' });
        attachReceiptToTask(state, action.taskId, receipt);
        return receipt;
      }
    }
    const receipt = unsupportedBrowserReceipt(action, 'DOM-aware browser automation is disabled in this environment; enable ACU_ENABLE_PLAYWRIGHT=1 to attempt the Playwright adapter.');
    pushHistory(state, sessionId, { action: action as unknown as JsonObject, receipt: receipt as unknown as JsonObject, source: 'browser-disabled' });
    attachReceiptToTask(state, action.taskId, receipt);
    return receipt;
  }

  const browser = await ensureBrowser(state, sessionId);
  const startedAt = new Date().toISOString();
  const receiptId = randomUUID();
  let result: JsonObject = {};
  const artifacts: { kind: string; path: string; mime_type?: string }[] = [];

  switch (action.kind) {
    case 'browser_open':
      await browser.page.goto(action.url, { waitUntil: 'domcontentloaded' });
      result = { url: browser.page.url(), title: await browser.page.title() };
      break;
    case 'browser_get_dom':
      result = {
        current_url: browser.page.url(),
        title: await browser.page.title(),
        dom_html: await browser.page.content(),
        console_logs: [],
        network_events: [],
      };
      break;
    case 'browser_click':
      if (action.selector) {
        await browser.page.locator(action.selector).first().click();
        result = { mode: 'selector', selector: action.selector };
      } else if (typeof action.x === 'number' && typeof action.y === 'number') {
        const payload = toGuestAction({ kind: 'mouse_click', button: action.button, x: action.x, y: action.y, taskId: action.taskId });
        const receipt = await guestJson<ActionReceipt>(state, `/api/sessions/${sessionId}/actions`, { method: 'POST', body: JSON.stringify(payload) });
        pushHistory(state, sessionId, { action: action as unknown as JsonObject, receipt: receipt as unknown as JsonObject, source: 'browser-fallback' });
        return receipt;
      } else {
        throw new Error('browser_click requires selector or coordinates');
      }
      break;
    case 'browser_type':
      if (action.selector) {
        const locator = browser.page.locator(action.selector).first();
        await locator.click();
        await locator.fill(action.text);
        result = { mode: 'selector', selector: action.selector };
      } else {
        const payload = toGuestAction({ kind: 'type_text', text: action.text, taskId: action.taskId });
        const receipt = await guestJson<ActionReceipt>(state, `/api/sessions/${sessionId}/actions`, { method: 'POST', body: JSON.stringify(payload) });
        pushHistory(state, sessionId, { action: action as unknown as JsonObject, receipt: receipt as unknown as JsonObject, source: 'browser-fallback' });
        return receipt;
      }
      break;
    case 'browser_screenshot': {
      const path = join(artifactRoot, `${sessionId}-${Date.now()}-browser.png`);
      await mkdir(dirname(path), { recursive: true });
      await browser.page.screenshot({ path, fullPage: true });
      result = { path };
      artifacts.push({ kind: 'browser_screenshot', path, mime_type: 'image/png' });
      break;
    }
  }

  const receipt: ActionReceipt = {
    status: 'ok',
    receipt_id: receiptId,
    action_type: action.kind,
    started_at: startedAt,
    completed_at: new Date().toISOString(),
    result,
    artifacts,
    error: null,
  };
  pushHistory(state, sessionId, { action: action as unknown as JsonObject, receipt: receipt as unknown as JsonObject, source: 'browser-adapter' });
  attachReceiptToTask(state, action.taskId, receipt);
  return receipt;
}

async function serveStatic(req: IncomingMessage, res: ServerResponse): Promise<boolean> {
  const url = new URL(req.url ?? '/', 'http://127.0.0.1');
  if (!['GET', 'HEAD'].includes(req.method ?? 'GET')) return false;
  const relativePath = url.pathname === '/' ? 'index.html' : url.pathname.slice(1);
  const filePath = resolve(uiRoot, relativePath);
  if (!filePath.startsWith(uiRoot) || !existsSync(filePath)) return false;
  const info = await stat(filePath);
  const contentType = extname(filePath) === '.html'
    ? 'text/html; charset=utf-8'
    : extname(filePath) === '.css'
      ? 'text/css; charset=utf-8'
      : extname(filePath) === '.js'
        ? 'application/javascript; charset=utf-8'
        : 'application/octet-stream';
  res.statusCode = 200;
  res.setHeader('content-type', contentType);
  res.setHeader('content-length', String(info.size));
  if (req.method === 'HEAD') {
    res.end();
  } else {
    createReadStream(filePath).pipe(res);
  }
  return true;
}

export function createRequestHandler(state: ControlPlaneState) {
  return async (req: IncomingMessage, res: ServerResponse): Promise<void> => {
    try {
      if (await serveStatic(req, res)) return;
      const url = new URL(req.url ?? '/', 'http://127.0.0.1');

      if (req.method === 'GET' && url.pathname === '/api/health') {
        const health = await guestJson<JsonObject>(state, '/health');
        json(res, 200, { ok: true, guest: health, now: new Date().toISOString() });
        return;
      }

      if (req.method === 'GET' && url.pathname === '/api/adapters') {
        json(res, 200, {
          adapters: [
            { name: 'browser', structured: playwrightEnabled, fallback: 'desktop/browser_open + coordinate actions' },
            { name: 'terminal', structured: true, fallback: 'run_command/read_file/write_file' },
            { name: 'generic-desktop', structured: true, fallback: null }
          ]
        });
        return;
      }

      if (req.method === 'POST' && url.pathname === '/api/sessions') {
        const body = await readJson(req);
        const payload = await guestJson<{ session: SessionRecord }>(state, '/api/sessions', {
          method: 'POST',
          body: JSON.stringify(body),
        });
        json(res, 201, payload);
        return;
      }

      const sessionMatch = url.pathname.match(/^\/api\/sessions\/([^/]+)$/);
      if (sessionMatch && req.method === 'GET') {
        const payload = await guestJson<{ session: SessionRecord }>(state, `/api/sessions/${sessionMatch[1]}`);
        json(res, 200, { ...payload, browser_adapter: state.browserStates.has(sessionMatch[1]) });
        return;
      }
      if (sessionMatch && req.method === 'DELETE') {
        await closeBrowser(state, sessionMatch[1]);
        const payload = await guestJson<{ ok: boolean }>(state, `/api/sessions/${sessionMatch[1]}`, { method: 'DELETE' });
        state.actionHistory.delete(sessionMatch[1]);
        json(res, 200, payload);
        return;
      }

      const screenshotMatch = url.pathname.match(/^\/api\/sessions\/([^/]+)\/screenshot$/);
      if (screenshotMatch && req.method === 'GET') {
        const upstream = await fetch(`${state.guestRuntimeUrl}/api/sessions/${screenshotMatch[1]}/screenshot`);
        res.statusCode = upstream.status;
        upstream.headers.forEach((value, key) => res.setHeader(key, value));
        res.end(Buffer.from(await upstream.arrayBuffer()));
        return;
      }

      const observationMatch = url.pathname.match(/^\/api\/sessions\/([^/]+)\/observation$/);
      if (observationMatch && req.method === 'GET') {
        const sessionId = observationMatch[1];
        const payload = await guestJson<Record<string, unknown>>(state, `/api/sessions/${sessionId}/observation`);
        payload.screenshot_url = `/api/sessions/${sessionId}/screenshot?ts=${Date.now()}`;
        payload.action_history = state.actionHistory.get(sessionId) ?? [];
        json(res, 200, payload);
        return;
      }

      const actionsMatch = url.pathname.match(/^\/api\/sessions\/([^/]+)\/actions$/);
      if (actionsMatch && req.method === 'GET') {
        const sessionId = actionsMatch[1];
        const payload = await guestJson<RuntimeCapabilities>(state, `/api/sessions/${sessionId}/actions`);
        json(res, 200, {
          ...payload,
          browser_mode: playwrightEnabled ? payload.browser_mode : 'desktop_fallback',
          browser_adapter_enabled: playwrightEnabled,
          browser_adapter: ['browser_open', 'browser_get_dom', 'browser_click', 'browser_type', 'browser_screenshot'],
        });
        return;
      }
      if (actionsMatch && req.method === 'POST') {
        const sessionId = actionsMatch[1];
        const action = await readJson(req) as ActionRequest;
        let receipt: ActionReceipt;
        if (action.kind.startsWith('browser_')) {
          receipt = await handleBrowserAction(state, sessionId, action as BrowserAction);
        } else {
          receipt = await guestJson<ActionReceipt>(state, `/api/sessions/${sessionId}/actions`, {
            method: 'POST',
            body: JSON.stringify(toGuestAction(action)),
          });
          pushHistory(state, sessionId, { action: action as unknown as JsonObject, receipt: receipt as unknown as JsonObject, source: 'guest-runtime' });
          attachReceiptToTask(state, action.taskId, receipt);
        }
        json(res, 200, receipt);
        return;
      }

      if (req.method === 'POST' && url.pathname === '/api/tasks') {
        const body = await readJson(req);
        const now = new Date().toISOString();
        const task: TaskRecord = {
          id: randomUUID(),
          sessionId: String(body.session_id ?? ''),
          description: String(body.description ?? 'Untitled task'),
          status: 'running',
          createdAt: now,
          updatedAt: now,
          thoughtSummary: typeof body.thought_summary === 'string' ? body.thought_summary : undefined,
          requireApproval: Boolean(body.require_approval),
        };
        state.tasks.set(task.id, task);
        json(res, 201, { task });
        return;
      }

      const taskMatch = url.pathname.match(/^\/api\/tasks\/([^/]+)$/);
      if (taskMatch && req.method === 'GET') {
        const task = state.tasks.get(taskMatch[1]);
        if (!task) {
          json(res, 404, { error: 'task not found' });
          return;
        }
        json(res, 200, { task });
        return;
      }

      const taskActionMatch = url.pathname.match(/^\/api\/tasks\/([^/]+)\/(pause|resume|terminate|complete|reset)$/);
      if (taskActionMatch && req.method === 'POST') {
        const task = state.tasks.get(taskActionMatch[1]);
        if (!task) {
          json(res, 404, { error: 'task not found' });
          return;
        }
        task.status = taskActionMatch[2] === 'reset' ? 'pending' : mapTaskStatus(taskActionMatch[2]);
        task.updatedAt = new Date().toISOString();
        state.tasks.set(task.id, task);
        json(res, 200, { task });
        return;
      }

      if (req.method === 'GET' && url.pathname === '/api/dashboard') {
        json(res, 200, {
          tasks: Array.from(state.tasks.values()),
          action_history: Object.fromEntries(state.actionHistory.entries()),
        });
        return;
      }

      json(res, 404, { error: 'not found', path: url.pathname });
    } catch (error) {
      json(res, 500, { error: error instanceof Error ? error.message : String(error) });
    }
  };
}

export async function startControlPlaneServer(port = Number(process.env.PORT ?? 3000), guestRuntimeUrl = defaultGuestRuntimeUrl): Promise<{ server: Server; state: ControlPlaneState }> {
  await mkdir(artifactRoot, { recursive: true });
  const state: ControlPlaneState = {
    guestRuntimeUrl,
    tasks: new Map(),
    actionHistory: new Map(),
    browserStates: new Map(),
    browserSnapshots: new Map(),
  };
  const handler = createRequestHandler(state);
  const server = createServer((req, res) => void handler(req, res));
  await new Promise<void>((resolvePromise) => server.listen(port, resolvePromise));
  return { server, state };
}
