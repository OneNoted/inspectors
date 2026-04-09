import { buildScreenshotUrl, describeLiveDesktopView, getLiveDesktopView } from './live-view.js';
import { buildSessionUrl, getSessionIdFromLocation, parseSessionReference } from './session-link.js';

const sessionMeta = document.getElementById('session-meta');
const desktopImage = document.getElementById('desktop-image');
const desktopPanelTitle = document.getElementById('desktop-panel-title');
const liveViewBadge = document.getElementById('live-view-badge');
const liveViewTrust = document.getElementById('live-view-trust');
const desktopPlaceholder = document.getElementById('desktop-placeholder');
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
const existingSessionInput = document.getElementById('existing-session');
const qemuOptions = document.getElementById('qemu-options');
const sessionSummary = document.getElementById('session-summary');
let sessionId = null;
let taskId = null;
let liveViewUrl = null;

async function json(url, options) {
  const res = await fetch(url, { headers: { 'content-type': 'application/json' }, ...options });
  const text = await res.text();
  const payload = text ? JSON.parse(text) : null;
  if (!res.ok) {
    const errorMessage = payload?.error?.message ?? payload?.error ?? `${res.status} ${res.statusText}`;
    throw new Error(String(errorMessage));
  }
  return payload;
}

function updateProviderOptions() {
  const isQemu = providerSelect.value === 'qemu';
  qemuOptions.hidden = !isQemu;
}

function syncSessionLocation() {
  const nextUrl = buildSessionUrl(sessionId);
  window.history.replaceState({}, '', nextUrl);
  existingSessionInput.value = sessionId ?? '';
}

function resetDesktopState(message = 'Live desktop unavailable') {
  liveViewUrl = null;
  viewerFrame.hidden = true;
  viewerFrame.removeAttribute('src');
  viewerLink.hidden = true;
  desktopImage.hidden = true;
  desktopImage.removeAttribute('src');
  desktopPlaceholder.hidden = false;
  desktopPlaceholder.textContent = message;
  desktopPanelTitle.textContent = 'Live desktop view';
  liveViewBadge.textContent = 'Unavailable';
  liveViewTrust.textContent = 'No session selected.';
}

function summarizeSession(session) {
  const liveView = getLiveDesktopView(session);
  const parts = [
    `provider=${session.provider}`,
    `bridge=${session.bridge_status ?? 'n/a'}`,
    `ready=${session.readiness_state ?? 'n/a'}`,
    `view=${liveView.mode}/${liveView.status}`,
  ];
  if (session.qemu_profile) parts.push(`profile=${session.qemu_profile}`);
  sessionSummary.textContent = parts.join(' · ');
}

function updateLiveView(session) {
  const liveView = getLiveDesktopView(session);
  const description = describeLiveDesktopView(session);
  desktopPanelTitle.textContent = description.title;
  liveViewBadge.textContent = description.badge;
  liveViewTrust.textContent = description.trustText;
  desktopPlaceholder.textContent = description.placeholderText;
  desktopPlaceholder.hidden = !description.showPlaceholder;

  if (description.showFrame && liveView.canonical_url) {
    if (liveViewUrl !== liveView.canonical_url) {
      liveViewUrl = liveView.canonical_url;
      viewerFrame.src = liveView.canonical_url;
    }
    viewerFrame.hidden = false;
    desktopImage.hidden = true;
  } else {
    liveViewUrl = null;
    viewerFrame.hidden = true;
    viewerFrame.removeAttribute('src');
    desktopImage.hidden = !description.showImage;
  }

  if (description.showImage && liveView.mode === 'screenshot_poll') {
    const screenshotUrl = buildScreenshotUrl(liveView);
    if (screenshotUrl) desktopImage.src = screenshotUrl;
  }

  if (liveView.debug_url) {
    viewerLink.href = liveView.debug_url;
    viewerLink.textContent = description.debugLinkLabel ?? 'Open debug viewer';
    viewerLink.hidden = false;
  } else {
    viewerLink.hidden = true;
  }
}

async function refresh() {
  if (!sessionId) return;
  try {
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
    syncSessionLocation();
  } catch (error) {
    sessionSummary.textContent = `session=${sessionId} · unavailable`;
    sessionMeta.textContent = JSON.stringify({ error: String(error) }, null, 2);
    observation.textContent = JSON.stringify({ error: 'observation unavailable' }, null, 2);
    historyEl.textContent = '[]';
    tasksEl.textContent = '[]';
    resetDesktopState('Live desktop unavailable for the requested session');
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
document.getElementById('attach-session').addEventListener('click', async () => {
  const nextSessionId = parseSessionReference(existingSessionInput.value);
  if (!nextSessionId) {
    sessionSummary.textContent = 'Enter a session id or live-view URL to attach.';
    return;
  }
  sessionId = nextSessionId;
  taskId = null;
  await refresh();
});
document.getElementById('clear-session').addEventListener('click', () => {
  sessionId = null;
  taskId = null;
  existingSessionInput.value = '';
  syncSessionLocation();
  sessionSummary.textContent = 'No session';
  sessionMeta.textContent = 'No session';
  observation.textContent = '';
  historyEl.textContent = '';
  tasksEl.textContent = '';
  resetDesktopState();
});
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

desktopImage.addEventListener('error', () => {
  desktopPlaceholder.hidden = false;
  desktopPlaceholder.textContent = 'Screenshot fallback unavailable';
});

desktopImage.addEventListener('load', () => {
  if (!desktopImage.hidden) desktopPlaceholder.hidden = true;
});

sessionId = getSessionIdFromLocation();
if (sessionId) {
  existingSessionInput.value = sessionId;
  refresh().catch(() => {});
} else {
  resetDesktopState();
}

setInterval(() => { refresh().catch(() => {}); }, 3000);
