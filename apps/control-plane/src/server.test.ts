import test from 'node:test';
import assert from 'node:assert/strict';
import { toGuestAction } from './server.js';

test('toGuestAction maps taskId to task_id', () => {
  const payload = toGuestAction({ kind: 'type_text', text: 'hello', taskId: 'task-1' });
  assert.equal(payload.kind, 'type_text');
  assert.equal(payload.text, 'hello');
  assert.equal(payload.task_id, 'task-1');
});

test('toGuestAction forwards run_as_user fields for desktop-sensitive actions', () => {
  const openPayload = toGuestAction({
    kind: 'open_app',
    name: 'taskers',
    run_as_user: 'desktop',
    taskId: 'task-2',
  });
  assert.deepEqual(openPayload, {
    kind: 'open_app',
    name: 'taskers',
    run_as_user: 'desktop',
    task_id: 'task-2',
  });

  const commandPayload = toGuestAction({
    kind: 'run_command',
    command: 'echo hello',
    run_as_user: 'ubuntu',
  });
  assert.deepEqual(commandPayload, {
    kind: 'run_command',
    command: 'echo hello',
    cwd: null,
    env: null,
    run_as_user: 'ubuntu',
    task_id: null,
  });
});
