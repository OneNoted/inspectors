import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';

delete process.env.ACU_ENABLE_PLAYWRIGHT;

const { startControlPlaneServer } = await import('./server.js');

interface ReceiptPayload {
  status: 'ok' | 'error';
  receipt_id: string;
  action_type: string;
  started_at: string;
  completed_at: string;
  result: Record<string, unknown>;
  artifacts: { kind: string; path: string; mime_type?: string }[];
  error: null;
}

function okReceipt(actionType: string, result: Record<string, unknown> = {}): ReceiptPayload {
  const now = new Date().toISOString();
  return {
    status: 'ok',
    receipt_id: `receipt-${actionType}`,
    action_type: actionType,
    started_at: now,
    completed_at: now,
    result,
    artifacts: [],
    error: null,
  };
}

async function startFakeGuestRuntime() {
  const actionPayloads: Record<string, unknown>[] = [];
  const guestServer = createServer(async (req: IncomingMessage, res: ServerResponse) => {
    const url = new URL(req.url ?? '/', 'http://127.0.0.1');
    const chunks: Buffer[] = [];
    for await (const chunk of req) {
      chunks.push(Buffer.from(chunk));
    }
    const body = chunks.length > 0 ? JSON.parse(Buffer.concat(chunks).toString('utf8')) as Record<string, unknown> : {};

    if (req.method === 'GET' && url.pathname === '/health') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ status: 'ok' }));
      return;
    }
    if (req.method === 'GET' && url.pathname === '/api/sessions/qemu-viewer/actions') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({
        actions: [],
        provider: 'qemu',
        browser_mode: 'viewer_only',
        vm_mode: 'qemu',
        enrichments: ['viewer'],
      }));
      return;
    }
    if (req.method === 'POST' && url.pathname === '/api/sessions/qemu-viewer/actions') {
      actionPayloads.push(body);
      const kind = String(body.kind ?? 'unknown');
      const result = kind === 'browser_open' ? { url: body.url ?? null } : {};
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify(okReceipt(kind, result)));
      return;
    }

    res.writeHead(404, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ error: 'not found', path: url.pathname }));
  });

  await new Promise<void>((resolve) => guestServer.listen(0, resolve));
  const guestPort = (guestServer.address() as { port: number }).port;
  const guestUrl = `http://127.0.0.1:${guestPort}`;
  return { guestServer, guestUrl, actionPayloads };
}

async function startHarness() {
  const guest = await startFakeGuestRuntime();
  const controlPlane = await startControlPlaneServer(0, guest.guestUrl);
  const port = (controlPlane.server.address() as { port: number }).port;
  const baseUrl = `http://127.0.0.1:${port}`;
  return { ...guest, ...controlPlane, baseUrl };
}

async function stopHarness(harness: Awaited<ReturnType<typeof startHarness>>) {
  await new Promise<void>((resolve) => harness.server.close(() => resolve()));
  await new Promise<void>((resolve) => harness.guestServer.close(() => resolve()));
}

test('browser_open fallback forwards task_id and browser_get_dom reuses cached HTML when Playwright is disabled', async () => {
  const harness = await startHarness();
  try {
    const openResponse = await fetch(`${harness.baseUrl}/api/sessions/qemu-viewer/actions`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        kind: 'browser_open',
        url: 'data:text/html,%3Ch1%3EQEMU%20viewer%20fallback%3C%2Fh1%3E',
        taskId: 'task-1',
      }),
    });
    assert.equal(openResponse.status, 200);

    const openReceipt = await openResponse.json() as ReceiptPayload;
    assert.equal(openReceipt.status, 'ok');
    assert.equal(harness.actionPayloads.length, 1);
    assert.deepEqual(harness.actionPayloads[0], {
      kind: 'browser_open',
      url: 'data:text/html,%3Ch1%3EQEMU%20viewer%20fallback%3C%2Fh1%3E',
      task_id: 'task-1',
    });

    const domResponse = await fetch(`${harness.baseUrl}/api/sessions/qemu-viewer/actions`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ kind: 'browser_get_dom', taskId: 'task-1' }),
    });
    assert.equal(domResponse.status, 200);

    const domReceipt = await domResponse.json() as ReceiptPayload;
    assert.equal(domReceipt.status, 'ok');
    assert.match(String(domReceipt.result.dom_html), /QEMU viewer fallback/);
  } finally {
    await stopHarness(harness);
  }
});

test('actions endpoint advertises desktop fallback metadata while preserving upstream qemu capabilities', async () => {
  const harness = await startHarness();
  try {
    const response = await fetch(`${harness.baseUrl}/api/sessions/qemu-viewer/actions`);
    assert.equal(response.status, 200);

    const payload = await response.json() as Record<string, unknown>;
    assert.equal(payload.provider, 'qemu');
    assert.equal(payload.vm_mode, 'qemu');
    assert.equal(payload.browser_mode, 'desktop_fallback');
    assert.equal(payload.browser_adapter_enabled, false);
    assert.equal(payload.browser_adapter_backend, 'desktop-fallback');
    assert.deepEqual(payload.browser_adapter, [
      'browser_open',
      'browser_get_dom',
      'browser_click',
      'browser_type',
      'browser_screenshot',
    ]);
  } finally {
    await stopHarness(harness);
  }
});
