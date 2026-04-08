import test from 'node:test';
import assert from 'node:assert/strict';
import { ComputerUseClient } from './index.js';

test('client exposes expected methods', () => {
  const client = new ComputerUseClient('http://localhost:3000');
  assert.equal(typeof client.createSession, 'function');
  assert.equal(typeof client.performAction, 'function');
});
