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

test('describeLiveDesktopView makes unverified xvfb browser fallback explicit', () => {
  const session = {
    id: 'xvfb-fallback',
    provider: 'xvfb',
    live_desktop_view: {
      mode: 'screenshot_poll',
      status: 'ready',
      provider_surface: 'guest_xvfb_screenshot',
      matches_action_plane: true,
      canonical_url: '/api/sessions/xvfb/screenshot',
      debug_url: null,
      reason: 'xvfb is an honest local/dev screenshot fallback without a live desktop stream',
      refresh_interval_ms: 3000,
    },
  };

  const observation = {
    active_window: null,
    summary: {
      active_window: null,
    },
    action_history: [
      {
        action: {
          kind: 'browser_open',
          url: 'https://example.com',
        },
        source: 'browser-open-fallback',
      },
    ],
  };

  const description = describeLiveDesktopView(session, observation);
  assert.equal(description.badge, 'Fallback idle');
  assert.equal(description.showImage, false);
  assert.equal(description.showPlaceholder, true);
  assert.match(description.trustText, /Use QEMU product/i);
  assert.match(description.placeholderText, /No visible Xvfb window/i);
});

test('describeLiveDesktopView makes idle xvfb screenshot fallback explicit even without browser actions', () => {
  const session = {
    id: 'xvfb-idle',
    provider: 'xvfb',
    live_desktop_view: {
      mode: 'screenshot_poll',
      status: 'ready',
      provider_surface: 'guest_xvfb_screenshot',
      matches_action_plane: true,
      canonical_url: '/api/sessions/xvfb/screenshot',
      debug_url: null,
      reason: 'xvfb is an honest local/dev screenshot fallback without a live desktop stream',
      refresh_interval_ms: 3000,
    },
  };

  const description = describeLiveDesktopView(session, {
    active_window: null,
    summary: { active_window: null },
    action_history: [],
  });
  assert.equal(description.badge, 'Fallback ready');
  assert.equal(description.showImage, false);
  assert.equal(description.showPlaceholder, true);
  assert.match(description.placeholderText, /No visible Xvfb window/i);
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
