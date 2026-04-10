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
