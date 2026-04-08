import test from 'node:test';
import assert from 'node:assert/strict';
import { toGuestAction } from './server.js';

test('toGuestAction maps taskId to task_id', () => {
  const payload = toGuestAction({ kind: 'type_text', text: 'hello', taskId: 'task-1' });
  assert.equal(payload.kind, 'type_text');
  assert.equal(payload.text, 'hello');
  assert.equal(payload.task_id, 'task-1');
});
