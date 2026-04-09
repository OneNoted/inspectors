import test from 'node:test';
import assert from 'node:assert/strict';
import { buildScreenshotUrl, describeLiveDesktopView, getLiveDesktopView } from './live-view.js';

test('describeLiveDesktopView prefers canonical qemu product stream', () => {
  const session = {
    id: 'qemu-product',
    live_desktop_view: {
      mode: 'stream',
      status: 'ready',
      provider_surface: 'qemu_novnc',
      matches_action_plane: true,
      canonical_url: '/api/sessions/qemu-product/live-view/',
      debug_url: 'http://127.0.0.1:32771',
      reason: null,
      refresh_interval_ms: null,
    },
  };

  const description = describeLiveDesktopView(session);
  assert.equal(description.badge, 'Live desktop');
  assert.equal(description.showFrame, true);
  assert.equal(description.showImage, false);
  assert.match(description.trustText, /matches the session action plane/i);
});

test('describeLiveDesktopView keeps qemu regression in screenshot fallback mode', () => {
  const session = {
    id: 'qemu-regression',
    live_desktop_view: {
      mode: 'screenshot_poll',
      status: 'ready',
      provider_surface: 'guest_xvfb_screenshot',
      matches_action_plane: true,
      canonical_url: '/api/sessions/qemu-regression/screenshot',
      debug_url: 'http://127.0.0.1:32771',
      reason: 'qemu regression keeps the VM viewer as debug-only because the action plane runs inside guest xvfb',
      refresh_interval_ms: 3000,
    },
  };

  const description = describeLiveDesktopView(session);
  assert.equal(description.badge, 'Screenshot fallback');
  assert.equal(description.showFrame, false);
  assert.equal(description.showImage, true);
  assert.equal(description.debugLinkLabel, 'Open debug VM viewer');
});

test('getLiveDesktopView falls back to unavailable metadata when absent', () => {
  const liveView = getLiveDesktopView({ viewer_url: null });
  assert.equal(liveView.mode, 'unavailable');
  assert.equal(liveView.status, 'unavailable');
});

test('buildScreenshotUrl appends a cache-busting timestamp', () => {
  const url = buildScreenshotUrl({
    canonical_url: '/api/sessions/xvfb/screenshot',
  });
  assert.match(String(url), /^\/api\/sessions\/xvfb\/screenshot\?ts=\d+$/);
});
