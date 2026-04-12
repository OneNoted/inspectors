import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

test('compiled control-plane server lazy-loads playwright-core', () => {
  const distDir = dirname(fileURLToPath(import.meta.url)).replace('/src', '/dist');
  const compiled = readFileSync(join(distDir, 'server.js'), 'utf8');
  assert.doesNotMatch(compiled, /from 'playwright-core'/);
  assert.match(compiled, /import\('playwright-core'\)/);
});
