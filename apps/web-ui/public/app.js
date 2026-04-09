const sessionMeta = document.getElementById('session-meta');
const desktopImage = document.getElementById('desktop-image');
const desktopPanelTitle = document.getElementById('desktop-panel-title');
const viewerFrame = document.getElementById('viewer-frame');
const viewerLink = document.getElementById('viewer-link');
const observation = document.getElementById('observation');
const historyEl = document.getElementById('history');
const tasksEl = document.getElementById('tasks');
const taskDescription = document.getElementById('task-description');
const actionPayload = document.getElementById('action-payload');
const providerSelect = document.getElementById('session-provider');
const qemuProfileSelect = document.getElementById('qemu-profile');
const sharedHostPathInput = document.getElementById('shared-host-path');
const qemuOptions = document.getElementById('qemu-options');
const sessionSummary = document.getElementById('session-summary');
let sessionId = null;
let taskId = null;
let liveViewUrl = null;

async function json(url, options) {
  const res = await fetch(url, { headers: { 'content-type': 'application/json' }, ...options });
  return res.json();
}

function updateProviderOptions() {
  const isQemu = providerSelect.value === 'qemu';
  qemuOptions.hidden = !isQemu;
}

function summarizeSession(session) {
  const parts = [
    `provider=${session.provider}`,
    `bridge=${session.bridge_status ?? 'n/a'}`,
    `ready=${session.readiness_state ?? 'n/a'}`,
  ];
  if (session.qemu_profile) parts.push(`profile=${session.qemu_profile}`);
  if (session.viewer_url) parts.push('live-view=available');
  sessionSummary.textContent = parts.join(' · ');
}

function updateLiveView(session) {
  if (session?.viewer_url) {
    if (liveViewUrl !== session.viewer_url) {
      liveViewUrl = session.viewer_url;
      viewerFrame.src = session.viewer_url;
      viewerLink.href = session.viewer_url;
      viewerLink.hidden = false;
    }
    viewerFrame.hidden = false;
    desktopImage.hidden = true;
    desktopPanelTitle.textContent = 'Live VM view';
  } else {
    liveViewUrl = null;
    viewerFrame.hidden = true;
    viewerFrame.removeAttribute('src');
    viewerLink.hidden = true;
    desktopImage.hidden = false;
    desktopPanelTitle.textContent = 'Live desktop screenshot';
  }
}

async function refresh() {
  if (!sessionId) return;
  const sessionPayload = await json(`/api/sessions/${sessionId}`);
  const session = sessionPayload.session ?? sessionPayload;
  const obs = await json(`/api/sessions/${sessionId}/observation`);
  const dashboard = await json('/api/dashboard');
  summarizeSession(session);
  sessionMeta.textContent = JSON.stringify(sessionPayload, null, 2);
  observation.textContent = JSON.stringify(obs.summary ?? obs, null, 2);
  historyEl.textContent = JSON.stringify(obs.action_history ?? [], null, 2);
  tasksEl.textContent = JSON.stringify(dashboard.tasks ?? [], null, 2);
  updateLiveView(session);
  if (!session.viewer_url) {
    desktopImage.src = `/api/sessions/${sessionId}/screenshot?ts=${Date.now()}`;
  } else if ((session.readiness_state ?? '') === 'runtime_ready') {
    desktopImage.src = `/api/sessions/${sessionId}/screenshot?ts=${Date.now()}`;
  }
}

document.getElementById('create-session').addEventListener('click', async () => {
  const payload = {
    provider: providerSelect.value,
    width: 1440,
    height: 900,
  };
  if (providerSelect.value === 'qemu') {
    payload.qemu_profile = qemuProfileSelect.value;
    if (sharedHostPathInput.value.trim()) {
      payload.shared_host_path = sharedHostPathInput.value.trim();
    }
  }
  const response = await json('/api/sessions', { method: 'POST', body: JSON.stringify(payload) });
  sessionId = response.session?.id;
  taskId = null;
  await refresh();
});

document.getElementById('refresh-session').addEventListener('click', refresh);
providerSelect.addEventListener('change', updateProviderOptions);
updateProviderOptions();

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
