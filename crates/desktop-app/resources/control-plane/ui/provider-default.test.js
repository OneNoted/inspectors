import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const html = readFileSync(new URL('./index.html', import.meta.url), 'utf8');

test('session provider defaults to qemu', () => {
  assert.match(html, /<option value="qemu" selected>QEMU VM<\/option>/);
  assert.match(html, /<option value="xvfb">Local Xvfb<\/option>/);
});

test('qemu profile defaults to product', () => {
  assert.match(html, /<option value="product" selected>Product guest \(Ubuntu \+ GNOME\)<\/option>/);
  assert.match(html, /<option value="regression">Regression fixture<\/option>/);
});

test('shared host path is opt-in even when qemu is default', () => {
  assert.match(html, /<input id="shared-host-path" type="text" value="" placeholder="\.\.\/taskers" \/>/);
});

test('session picker is rendered for choosing running sessions', () => {
  assert.match(html, /<label for="session-picker">Choose running session<\/label>/);
  assert.match(html, /<select id="session-picker">/);
});

test('default session flow emphasizes the qemu happy path', () => {
  assert.match(html, /Default path: create a QEMU product session, watch the live desktop, and delete it when you are done\./);
  assert.match(html, /<button id="create-session" class="btn btn-primary">Start default session<\/button>/);
  assert.match(html, /<button id="delete-session" type="button" class="btn">Delete session<\/button>/);
});

test('advanced controls keep debug-only actions out of the default path', () => {
  assert.match(html, /<details class="collapsible" id="advanced-controls">/);
  assert.match(html, /<summary><span class="summary-label">Advanced \/ debug<\/span><\/summary>/);
  assert.match(html, /<button id="reclaim-storage" type="button" class="btn btn-ghost">Reclaim stale storage<\/button>/);
  assert.match(html, /<summary><span class="summary-label">Manual action \(advanced\)<\/span><\/summary>/);
});
