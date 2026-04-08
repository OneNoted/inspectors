const sessionMeta = document.getElementById('session-meta');
const desktopImage = document.getElementById('desktop-image');
const observation = document.getElementById('observation');
const historyEl = document.getElementById('history');
const tasksEl = document.getElementById('tasks');
const taskDescription = document.getElementById('task-description');
const actionPayload = document.getElementById('action-payload');
let sessionId = null;
let taskId = null;

async function json(url, options) {
  const res = await fetch(url, { headers: { 'content-type': 'application/json' }, ...options });
  return res.json();
}

async function refresh() {
  if (!sessionId) return;
  const session = await json(`/api/sessions/${sessionId}`);
  const obs = await json(`/api/sessions/${sessionId}/observation`);
  const dashboard = await json('/api/dashboard');
  sessionMeta.textContent = JSON.stringify(session, null, 2);
  observation.textContent = JSON.stringify(obs.summary ?? obs, null, 2);
  historyEl.textContent = JSON.stringify(obs.action_history ?? [], null, 2);
  tasksEl.textContent = JSON.stringify(dashboard.tasks ?? [], null, 2);
  desktopImage.src = `/api/sessions/${sessionId}/screenshot?ts=${Date.now()}`;
}

document.getElementById('create-session').addEventListener('click', async () => {
  const payload = await json('/api/sessions', { method: 'POST', body: JSON.stringify({ provider: 'xvfb', width: 1440, height: 900 }) });
  sessionId = payload.session?.id;
  await refresh();
});

document.getElementById('refresh-session').addEventListener('click', refresh);

document.getElementById('create-task').addEventListener('click', async () => {
  if (!sessionId) return;
  const payload = await json('/api/tasks', { method: 'POST', body: JSON.stringify({ session_id: sessionId, description: taskDescription.value }) });
  taskId = payload.task?.id;
  await refresh();
});

document.querySelectorAll('[data-task-action]').forEach((button) => {
  button.addEventListener('click', async () => {
    if (!taskId) return;
    await json(`/api/tasks/${taskId}/${button.dataset.taskAction}`, { method: 'POST' });
    await refresh();
  });
});

document.getElementById('run-action').addEventListener('click', async () => {
  if (!sessionId) return;
  await json(`/api/sessions/${sessionId}/actions`, { method: 'POST', body: actionPayload.value });
  await refresh();
});

setInterval(() => { refresh().catch(() => {}); }, 3000);
