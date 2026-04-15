import test from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

const { startControlPlaneServer } = await import('./server.js');

function tempDir(prefix: string): string {
  return mkdtempSync(join(tmpdir(), `${prefix}-`));
}

async function startGuestServer() {
  const guestServer = createServer(async (req, res) => {
    const url = new URL(req.url ?? '/', 'http://127.0.0.1');
    if (req.method === 'GET' && url.pathname === '/health') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ status: 'ok' }));
      return;
    }

    if (req.method === 'POST' && url.pathname === '/api/sessions') {
      const chunks: Buffer[] = [];
      for await (const chunk of req) chunks.push(Buffer.from(chunk));
      const body = JSON.parse(Buffer.concat(chunks).toString('utf8')) as Record<string, unknown>;
      const qemuProfile = body.qemu_profile === 'regression' ? 'regression' : 'product';
      res.writeHead(201, { 'content-type': 'application/json' });
      res.end(JSON.stringify({
        session: {
          id: qemuProfile === 'product' ? 'qemu-product' : 'qemu-regression',
          provider: body.provider ?? 'qemu',
          qemu_profile: qemuProfile,
          display: null,
          width: 1440,
          height: 900,
          state: 'running',
          created_at: new Date().toISOString(),
          artifacts_dir: `artifacts/runtime/${qemuProfile}`,
          capabilities: ['viewer', 'vm'],
          browser_command: 'firefox',
          desktop_user: qemuProfile === 'product' ? 'ubuntu' : null,
          desktop_home: qemuProfile === 'product' ? '/home/ubuntu' : null,
          desktop_runtime_dir: qemuProfile === 'product' ? '/run/user/1000' : null,
          runtime_base_url: 'http://127.0.0.1:4001',
          viewer_url: 'http://127.0.0.1:8006',
          live_desktop_view: null,
          bridge_status: 'runtime_ready',
          readiness_state: 'runtime_ready',
          bridge_error: null,
        },
      }));
      return;
    }

    if (req.method === 'POST' && url.pathname === '/api/storage/reclaim') {
      const chunks: Buffer[] = [];
      for await (const chunk of req) chunks.push(Buffer.from(chunk));
      const body = JSON.parse(Buffer.concat(chunks).toString('utf8')) as Record<string, unknown>;
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({
        mode: body.mode ?? 'report',
        candidate_count: 1,
        candidates: [
          {
            path: '/tmp/inspectors/runtime/stale-session',
            tier: 'runtime',
            kind: 'legacy_runtime',
            reason: 'legacy inspectors runtime directory without an active container reference',
          },
        ],
        reclaimed: body.mode === 'apply' ? ['/tmp/inspectors/runtime/stale-session'] : [],
      }));
      return;
    }

    res.writeHead(404, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ error: 'not found', path: url.pathname }));
  });

  guestServer.listen(0);
  await once(guestServer, 'listening');
  const port = (guestServer.address() as { port: number }).port;
  return {
    baseUrl: `http://127.0.0.1:${port}`,
    guestServer,
  };
}

async function stopServers(controlPlane: Awaited<ReturnType<typeof startControlPlaneServer>>, guestServer: ReturnType<typeof createServer>) {
  await new Promise<void>((resolve) => controlPlane.server.close(() => resolve()));
  await new Promise<void>((resolve) => guestServer.close(() => resolve()));
}

test('control-plane serves the bundled operator UI assets', async () => {
  const guest = await startGuestServer();
  const controlPlane = await startControlPlaneServer(0, guest.baseUrl);
  const baseUrl = `http://127.0.0.1:${(controlPlane.server.address() as { port: number }).port}`;

  try {
    const rootResponse = await fetch(`${baseUrl}/`);
    assert.equal(rootResponse.status, 200);
    const html = await rootResponse.text();
    assert.match(html, /inspectors — agent computer use/);
    assert.match(html, /Live desktop view/);

    const appResponse = await fetch(`${baseUrl}/app.js`);
    assert.equal(appResponse.status, 200);
    assert.match(await appResponse.text(), /desktop=\$\{session\.desktop_user\}/);
  } finally {
    await stopServers(controlPlane, guest.guestServer);
  }
});

test('control-plane honors ACU_UI_ROOT for packaged desktop assets', async () => {
  const uiRoot = tempDir('acu-ui-root');
  mkdirSync(join(uiRoot, 'nested'));
  writeFileSync(join(uiRoot, 'index.html'), '<!doctype html><title>Packaged UI</title><main>Packaged UI</main>');
  writeFileSync(join(uiRoot, 'nested', 'ok.js'), 'console.log("ok");');
  const previousUiRoot = process.env.ACU_UI_ROOT;
  process.env.ACU_UI_ROOT = uiRoot;

  const guest = await startGuestServer();
  const controlPlane = await startControlPlaneServer(0, guest.baseUrl);
  const baseUrl = `http://127.0.0.1:${(controlPlane.server.address() as { port: number }).port}`;

  try {
    const rootResponse = await fetch(`${baseUrl}/`);
    assert.equal(rootResponse.status, 200);
    assert.match(await rootResponse.text(), /Packaged UI/);
    const nestedResponse = await fetch(`${baseUrl}/nested/ok.js`);
    assert.equal(nestedResponse.status, 200);
    assert.match(await nestedResponse.text(), /console\.log/);
  } finally {
    if (previousUiRoot === undefined) {
      delete process.env.ACU_UI_ROOT;
    } else {
      process.env.ACU_UI_ROOT = previousUiRoot;
    }
    await stopServers(controlPlane, guest.guestServer);
    rmSync(uiRoot, { recursive: true, force: true });
  }
});

test('qemu product session creation preserves desktop user metadata and live view enrichment', async () => {
  const guest = await startGuestServer();
  const controlPlane = await startControlPlaneServer(0, guest.baseUrl);
  const baseUrl = `http://127.0.0.1:${(controlPlane.server.address() as { port: number }).port}`;

  try {
    const response = await fetch(`${baseUrl}/api/sessions`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ provider: 'qemu', qemu_profile: 'product' }),
    });
    assert.equal(response.status, 201);
    const payload = await response.json() as { session: Record<string, unknown> };
    assert.equal(payload.session.desktop_user, 'ubuntu');
    assert.equal(payload.session.desktop_home, '/home/ubuntu');
    assert.equal(payload.session.desktop_runtime_dir, '/run/user/1000');
    assert.deepEqual(payload.session.live_desktop_view, {
      mode: 'stream',
      status: 'ready',
      provider_surface: 'qemu_novnc',
      matches_action_plane: true,
      canonical_url: '/api/sessions/qemu-product/live-view/',
      debug_url: 'http://127.0.0.1:8006',
      reason: null,
      refresh_interval_ms: null,
    });
  } finally {
    await stopServers(controlPlane, guest.guestServer);
  }
});

test('control-plane proxies storage reclaim requests', async () => {
  const guest = await startGuestServer();
  const controlPlane = await startControlPlaneServer(0, guest.baseUrl);
  const baseUrl = `http://127.0.0.1:${(controlPlane.server.address() as { port: number }).port}`;

  try {
    const response = await fetch(`${baseUrl}/api/storage/reclaim`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ mode: 'apply' }),
    });
    assert.equal(response.status, 200);
    const payload = await response.json() as {
      mode: string;
      candidate_count: number;
      reclaimed: string[];
    };
    assert.equal(payload.mode, 'apply');
    assert.equal(payload.candidate_count, 1);
    assert.deepEqual(payload.reclaimed, ['/tmp/inspectors/runtime/stale-session']);
  } finally {
    await stopServers(controlPlane, guest.guestServer);
  }
});
