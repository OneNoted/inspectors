import test from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { connect } from 'node:net';
import { gzipSync } from 'node:zlib';

const { startControlPlaneServer } = await import('./server.js');

interface Harness {
  baseUrl: string;
  server: Awaited<ReturnType<typeof startControlPlaneServer>>['server'];
  guestServer: ReturnType<typeof createServer>;
  viewerServer: ReturnType<typeof createServer>;
  viewerUrl: string;
  getLastUpgrade(): { path: string | null; host: string | null };
}

function sessionRecord(id: string, overrides: Record<string, unknown> = {}) {
  return {
    id,
    provider: 'qemu',
    qemu_profile: 'product',
    display: null,
    width: 1440,
    height: 900,
    state: 'running',
    created_at: new Date().toISOString(),
    artifacts_dir: `artifacts/runtime/${id}`,
    capabilities: ['viewer', 'vm'],
    browser_command: 'firefox',
    runtime_base_url: 'http://127.0.0.1:4001',
    viewer_url: null,
    live_desktop_view: null,
    review_recording: {
      mode: 'sparse_timeline',
      status: 'active',
      retention: 'ephemeral_until_export',
      event_count: 1,
      screenshot_count: 0,
      approx_bytes: 128,
      last_captured_at: new Date().toISOString(),
      exportable: true,
      exported_bundle: null,
      postmortem_retained_until: null,
      reason: null,
    },
    bridge_status: 'runtime_ready',
    readiness_state: 'runtime_ready',
    bridge_error: null,
    ...overrides,
  };
}

async function startHarness(): Promise<Harness> {
  let lastUpgradePath: string | null = null;
  let lastUpgradeHost: string | null = null;

  const viewerServer = createServer((req: IncomingMessage, res: ServerResponse) => {
    if (req.url === '/' || req.url === '/index.html') {
      const body = gzipSync('<!doctype html><title>Viewer Root</title><script src="app/ui.js"></script>');
      res.writeHead(200, {
        'content-type': 'text/html; charset=utf-8',
        'content-encoding': 'gzip',
        'content-length': String(body.length),
      });
      res.end(body);
      return;
    }
    if (req.url === '/app/ui.js') {
      res.writeHead(200, { 'content-type': 'application/javascript; charset=utf-8' });
      res.end('console.log("viewer asset");');
      return;
    }
    if (req.url === '/defaults.json') {
      res.writeHead(200, { 'content-type': 'application/json; charset=utf-8' });
      res.end(JSON.stringify({ resize: 'scale' }));
      return;
    }
    if (req.url === '/mandatory.json') {
      res.writeHead(200, { 'content-type': 'application/json; charset=utf-8' });
      res.end(JSON.stringify({ reconnect: true, autoconnect: true }));
      return;
    }
    res.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' });
    res.end('missing');
  });
  viewerServer.on('upgrade', (req, socket) => {
    lastUpgradePath = req.url ?? null;
    lastUpgradeHost = typeof req.headers.host === 'string' ? req.headers.host : null;
    socket.write([
      'HTTP/1.1 101 Switching Protocols',
      'Upgrade: websocket',
      'Connection: Upgrade',
      '',
      '',
    ].join('\r\n'));
    socket.write('upstream-established');
    socket.end();
  });
  viewerServer.listen(0);
  await once(viewerServer, 'listening');
  const viewerPort = (viewerServer.address() as { port: number }).port;
  const viewerUrl = `http://127.0.0.1:${viewerPort}`;

  const guestServer = createServer((req: IncomingMessage, res: ServerResponse) => {
    const url = new URL(req.url ?? '/', 'http://127.0.0.1');
    if (req.method === 'GET' && url.pathname === '/health') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ status: 'ok' }));
      return;
    }
    if (req.method === 'GET' && url.pathname === '/api/sessions/qemu-product') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({
        session: sessionRecord('qemu-product', {
          provider: 'qemu',
          qemu_profile: 'product',
          viewer_url: viewerUrl,
        }),
      }));
      return;
    }
    if (req.method === 'GET' && url.pathname === '/api/sessions') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({
        sessions: [
          sessionRecord('qemu-product', {
            provider: 'qemu',
            qemu_profile: 'product',
            viewer_url: viewerUrl,
          }),
          sessionRecord('xvfb', {
            provider: 'xvfb',
            qemu_profile: null,
            viewer_url: null,
            display: ':90',
            capabilities: ['screenshot'],
          }),
        ],
      }));
      return;
    }
    if (req.method === 'GET' && url.pathname === '/api/sessions/qemu-regression') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({
        session: sessionRecord('qemu-regression', {
          provider: 'qemu',
          qemu_profile: 'regression',
          viewer_url: viewerUrl,
          review_recording: {
            mode: 'unavailable',
            status: 'unavailable',
            retention: 'ephemeral_until_export',
            event_count: 0,
            screenshot_count: 0,
            approx_bytes: 0,
            last_captured_at: null,
            exportable: false,
            exported_bundle: null,
            postmortem_retained_until: null,
            reason: 'review recording is available only for qemu product sessions in v1',
          },
        }),
      }));
      return;
    }
    if (req.method === 'GET' && url.pathname === '/api/sessions/xvfb') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({
        session: sessionRecord('xvfb', {
          provider: 'xvfb',
          qemu_profile: null,
          viewer_url: null,
          display: ':90',
          capabilities: ['screenshot'],
          review_recording: {
            mode: 'unavailable',
            status: 'unavailable',
            retention: 'ephemeral_until_export',
            event_count: 0,
            screenshot_count: 0,
            approx_bytes: 0,
            last_captured_at: null,
            exportable: false,
            exported_bundle: null,
            postmortem_retained_until: null,
            reason: 'review recording is available only for qemu product sessions in v1',
          },
        }),
      }));
      return;
    }
    if (req.method === 'POST' && url.pathname === '/api/sessions/qemu-product/review/export') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({
        bundle: {
          kind: 'review_bundle',
          path: 'artifacts/exports/qemu-product-review',
          mime_type: null,
        },
        review_recording: {
          mode: 'sparse_timeline',
          status: 'exported',
          retention: 'ephemeral_until_export',
          event_count: 4,
          screenshot_count: 2,
          approx_bytes: 512,
          last_captured_at: new Date().toISOString(),
          exportable: true,
          exported_bundle: {
            kind: 'review_bundle',
            path: 'artifacts/exports/qemu-product-review',
            mime_type: null,
          },
          postmortem_retained_until: null,
          reason: null,
        },
      }));
      return;
    }
    if (req.method === 'GET' && url.pathname === '/api/sessions/missing') {
      res.writeHead(404, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: 'session not found' }));
      return;
    }
    if (req.method === 'GET' && url.pathname === '/api/sessions/xvfb/screenshot') {
      res.writeHead(200, { 'content-type': 'image/png' });
      res.end(Buffer.from([0x89, 0x50, 0x4e, 0x47]));
      return;
    }
    if (req.method === 'GET' && url.pathname === '/api/sessions/stale/screenshot') {
      res.writeHead(404, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: 'session not found', code: 'session_not_found' }));
      return;
    }
    res.writeHead(404, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ error: 'not found', path: url.pathname }));
  });
  guestServer.listen(0);
  await once(guestServer, 'listening');
  const guestPort = (guestServer.address() as { port: number }).port;

  const controlPlane = await startControlPlaneServer(0, `http://127.0.0.1:${guestPort}`);
  const port = (controlPlane.server.address() as { port: number }).port;

  return {
    baseUrl: `http://127.0.0.1:${port}`,
    server: controlPlane.server,
    guestServer,
    viewerServer,
    viewerUrl,
    getLastUpgrade() {
      return { path: lastUpgradePath, host: lastUpgradeHost };
    },
  };
}

async function stopHarness(harness: Harness) {
  await new Promise<void>((resolve) => harness.server.close(() => resolve()));
  await new Promise<void>((resolve) => harness.guestServer.close(() => resolve()));
  await new Promise<void>((resolve) => harness.viewerServer.close(() => resolve()));
}

test('session metadata exposes truthful live_desktop_view modes', async () => {
  const harness = await startHarness();
  try {
    const qemuProduct = await fetch(`${harness.baseUrl}/api/sessions/qemu-product`).then((res) => res.json()) as { session: any };
    assert.equal(qemuProduct.session.live_desktop_view.mode, 'stream');
    assert.equal(qemuProduct.session.live_desktop_view.canonical_url, '/api/sessions/qemu-product/live-view/');
    assert.equal(qemuProduct.session.live_desktop_view.debug_url, harness.viewerUrl);
    assert.equal(qemuProduct.session.live_desktop_view.matches_action_plane, true);
    assert.equal(qemuProduct.session.review_recording.mode, 'sparse_timeline');
    assert.equal(qemuProduct.session.review_recording.exportable, true);

    const qemuRegression = await fetch(`${harness.baseUrl}/api/sessions/qemu-regression`).then((res) => res.json()) as { session: any };
    assert.equal(qemuRegression.session.live_desktop_view.mode, 'screenshot_poll');
    assert.equal(qemuRegression.session.live_desktop_view.canonical_url, '/api/sessions/qemu-regression/screenshot');
    assert.equal(qemuRegression.session.live_desktop_view.debug_url, harness.viewerUrl);
    assert.equal(qemuRegression.session.review_recording.mode, 'unavailable');

    const xvfb = await fetch(`${harness.baseUrl}/api/sessions/xvfb`).then((res) => res.json()) as { session: any };
    assert.equal(xvfb.session.live_desktop_view.mode, 'screenshot_poll');
    assert.equal(xvfb.session.live_desktop_view.canonical_url, '/api/sessions/xvfb/screenshot');
    assert.match(String(xvfb.session.live_desktop_view.reason), /screenshot fallback/i);
    assert.equal(xvfb.session.review_recording.mode, 'unavailable');
  } finally {
    await stopHarness(harness);
  }
});

test('session list enriches live_desktop_view metadata for the picker', async () => {
  const harness = await startHarness();
  try {
    const payload = await fetch(`${harness.baseUrl}/api/sessions`).then((res) => res.json()) as { sessions: any[] };
    assert.equal(payload.sessions.length, 2);
    assert.equal(payload.sessions[0].id, 'qemu-product');
    assert.equal(payload.sessions[0].live_desktop_view.mode, 'stream');
    assert.equal(payload.sessions[1].id, 'xvfb');
    assert.equal(payload.sessions[1].live_desktop_view.mode, 'screenshot_poll');
  } finally {
    await stopHarness(harness);
  }
});

test('screenshot route preserves success and stale-session failures', async () => {
  const harness = await startHarness();
  try {
    const screenshotResponse = await fetch(`${harness.baseUrl}/api/sessions/xvfb/screenshot`);
    assert.equal(screenshotResponse.status, 200);
    assert.equal(screenshotResponse.headers.get('content-type'), 'image/png');
    assert.equal((await screenshotResponse.arrayBuffer()).byteLength, 4);

    const staleResponse = await fetch(`${harness.baseUrl}/api/sessions/stale/screenshot`);
    assert.equal(staleResponse.status, 404);
    assert.match(String(staleResponse.headers.get('content-type')), /application\/json/);
    const stalePayload = await staleResponse.json() as Record<string, unknown>;
    assert.equal(stalePayload.error, 'session not found');
  } finally {
    await stopHarness(harness);
  }
});

test('live-view route proxies viewer assets and rejects non-stream sessions', async () => {
  const harness = await startHarness();
  try {
    const liveViewRoot = await fetch(`${harness.baseUrl}/api/sessions/qemu-product/live-view/`);
    assert.equal(liveViewRoot.status, 200);
    assert.equal(liveViewRoot.headers.get('content-encoding'), null);
    assert.match(await liveViewRoot.text(), /Viewer Root/);

    const liveViewDefaults = await fetch(`${harness.baseUrl}/api/sessions/qemu-product/live-view/defaults.json`);
    assert.equal(liveViewDefaults.status, 200);
    const defaultsPayload = await liveViewDefaults.json() as { path?: string };
    assert.equal(defaultsPayload.path, '/api/sessions/qemu-product/live-view/websockify');

    const liveViewAsset = await fetch(`${harness.baseUrl}/api/sessions/qemu-product/live-view/app/ui.js`);
    assert.equal(liveViewAsset.status, 200);
    assert.match(await liveViewAsset.text(), /viewer asset/);

    const fallbackResponse = await fetch(`${harness.baseUrl}/api/sessions/xvfb/live-view/`);
    assert.equal(fallbackResponse.status, 409);
    const fallbackPayload = await fallbackResponse.json() as { error: { code: string } };
    assert.equal(fallbackPayload.error.code, 'live_desktop_view_unavailable');
  } finally {
    await stopHarness(harness);
  }
});

test('live-view websocket upgrades proxy to the upstream viewer', async () => {
  const harness = await startHarness();
  try {
    const port = Number(new URL(harness.baseUrl).port);
    const socket = connect(port, '127.0.0.1');
    const chunks: Buffer[] = [];
    socket.on('data', (chunk) => chunks.push(Buffer.from(chunk)));
    socket.write([
      'GET /api/sessions/qemu-product/live-view/websockify HTTP/1.1',
      'Host: 127.0.0.1',
      'Connection: Upgrade',
      'Upgrade: websocket',
      'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==',
      'Sec-WebSocket-Version: 13',
      '',
      '',
    ].join('\r\n'));
    await once(socket, 'close');
    const payload = Buffer.concat(chunks).toString('utf8');
    assert.match(payload, /101 Switching Protocols/);
    assert.match(payload, /upstream-established/);
    assert.deepEqual(harness.getLastUpgrade(), {
      path: '/websockify',
      host: new URL(harness.viewerUrl).host,
    });
  } finally {
    await stopHarness(harness);
  }
});

test('review export route proxies durable review bundle metadata', async () => {
  const harness = await startHarness();
  try {
    const response = await fetch(`${harness.baseUrl}/api/sessions/qemu-product/review/export`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({}),
    });
    assert.equal(response.status, 200);
    const payload = await response.json() as { bundle: { kind: string; path: string }; review_recording: { status: string } };
    assert.equal(payload.bundle.kind, 'review_bundle');
    assert.equal(payload.bundle.path, 'artifacts/exports/qemu-product-review');
    assert.equal(payload.review_recording.status, 'exported');
  } finally {
    await stopHarness(harness);
  }
});
