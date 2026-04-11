import { buildScreenshotUrl, describeLiveDesktopView, getLiveDesktopView } from './live-view.js';
import { buildSessionUrl, getSessionIdFromLocation, parseSessionReference } from './session-link.js';

const sessionMeta = document.getElementById('session-meta');
const desktopImage = document.getElementById('desktop-image');
const desktopPanelTitle = document.getElementById('desktop-panel-title');
const liveViewBadge = document.getElementById('live-view-badge');
const liveViewTrust = document.getElementById('live-view-trust');
const desktopPlaceholder = document.getElementById('desktop-placeholder');
const desktopHistoryPanel = document.getElementById('desktop-history-panel');
const desktopHistoryStrip = document.getElementById('desktop-history-strip');
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
const sessionPicker = document.getElementById('session-picker');
const qemuOptions = document.getElementById('qemu-options');
const sessionSummary = document.getElementById('session-summary');
let sessionId = null;
let taskId = null;
let liveViewUrl = null;
let screenshotHistory = [];
let screenshotHistorySessionId = null;

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
  if (sessionPicker && sessionPicker.value !== (sessionId ?? '')) {
    sessionPicker.value = sessionId ?? '';
  }
}

function resetDesktopState(message = 'Live desktop unavailable') {
  liveViewUrl = null;
  viewerFrame.hidden = true;
  viewerFrame.removeAttribute('src');
  viewerLink.hidden = true;
  desktopImage.hidden = true;
  desktopImage.removeAttribute('src');
  clearScreenshotHistory();
  desktopPlaceholder.hidden = false;
  desktopPlaceholder.textContent = message;
  desktopPanelTitle.textContent = 'Live desktop view';
  liveViewBadge.textContent = 'Unavailable';
  liveViewTrust.textContent = 'No session selected.';
}

function clearScreenshotHistory() {
  const previousMainUrl = desktopImage.dataset.objectUrl;
  if (previousMainUrl) {
    URL.revokeObjectURL(previousMainUrl);
    delete desktopImage.dataset.objectUrl;
  }
  screenshotHistory.forEach((entry) => URL.revokeObjectURL(entry.url));
  screenshotHistory = [];
  screenshotHistorySessionId = null;
  if (desktopHistoryStrip) {
    desktopHistoryStrip.replaceChildren();
  }
  if (desktopHistoryPanel) {
    desktopHistoryPanel.hidden = true;
  }
}

function renderScreenshotHistory() {
  if (!desktopHistoryStrip || !desktopHistoryPanel) return;
  desktopHistoryStrip.replaceChildren();
  for (const entry of screenshotHistory) {
    const card = document.createElement('article');
    card.className = 'desktop-history-entry';

    const image = document.createElement('img');
    image.src = entry.url;
    image.alt = `Fallback screenshot captured ${entry.label}`;

    const label = document.createElement('span');
    label.textContent = entry.label;

    card.append(image, label);
    desktopHistoryStrip.append(card);
  }
  desktopHistoryPanel.hidden = screenshotHistory.length === 0;
}

async function refreshScreenshotHistory(liveView, currentSessionId) {
  const screenshotUrl = buildScreenshotUrl(liveView);
  if (!screenshotUrl) return;
  if (screenshotHistorySessionId !== currentSessionId) {
    clearScreenshotHistory();
    screenshotHistorySessionId = currentSessionId;
  }

  const response = await fetch(screenshotUrl);
  if (!response.ok) {
    throw new Error(`screenshot request failed: ${response.status} ${response.statusText}`);
  }
  const blob = await response.blob();
  const mainUrl = URL.createObjectURL(blob);
  const historyUrl = URL.createObjectURL(blob);
  const previousMainUrl = desktopImage.dataset.objectUrl;
  if (previousMainUrl) URL.revokeObjectURL(previousMainUrl);
  desktopImage.dataset.objectUrl = mainUrl;
  desktopImage.src = mainUrl;
  const previousEntries = screenshotHistory;
  screenshotHistory.unshift({
    url: historyUrl,
    label: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }),
  });
  screenshotHistory = screenshotHistory.slice(0, 6);
  previousEntries.slice(5).forEach((entry) => URL.revokeObjectURL(entry.url));
  renderScreenshotHistory();
}

function sessionOptionLabel(session) {
  const parts = [session.id.slice(0, 8), session.provider];
  if (session.qemu_profile) parts.push(session.qemu_profile);
  parts.push(session.readiness_state ?? session.state ?? 'unknown');
  return parts.join(' · ');
}

async function refreshSessionPicker() {
  if (!sessionPicker) return;
  try {
    const payload = await json('/api/sessions');
    const sessions = Array.isArray(payload.sessions) ? payload.sessions : [];
    sessions.sort((left, right) => String(right.created_at ?? '').localeCompare(String(left.created_at ?? '')));

    sessionPicker.replaceChildren();
    const placeholder = document.createElement('option');
    placeholder.value = '';
    placeholder.textContent = sessions.length > 0 ? 'Select a running session' : 'No running sessions';
    sessionPicker.append(placeholder);

    for (const session of sessions) {
      const option = document.createElement('option');
      option.value = session.id;
      option.textContent = sessionOptionLabel(session);
      sessionPicker.append(option);
    }

    sessionPicker.disabled = sessions.length === 0;
    sessionPicker.value = sessionId && sessions.some((session) => session.id === sessionId)
      ? sessionId
      : '';
  } catch (error) {
    sessionPicker.replaceChildren();
    const option = document.createElement('option');
    option.value = '';
    option.textContent = 'Sessions unavailable';
    sessionPicker.append(option);
    sessionPicker.disabled = true;
  }
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
  if (session.desktop_user) parts.push(`desktop=${session.desktop_user}`);
  sessionSummary.textContent = parts.join(' · ');
}

async function updateLiveView(session, observation) {
  const liveView = getLiveDesktopView(session);
  const description = describeLiveDesktopView(session, observation);
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
    await refreshScreenshotHistory(liveView, session.id);
  } else {
    clearScreenshotHistory();
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
  await refreshSessionPicker();
  if (!sessionId) return;
  try {
    const sessionPayload = await json(`/api/sessions/${sessionId}`);
    const session = sessionPayload.session ?? sessionPayload;
    summarizeSession(session);
    sessionMeta.textContent = JSON.stringify(sessionPayload, null, 2);
    await updateLiveView(session, null);
    syncSessionLocation();

    try {
      const obs = await json(`/api/sessions/${sessionId}/observation`);
      observation.textContent = JSON.stringify(obs.summary ?? obs, null, 2);
      historyEl.textContent = JSON.stringify(obs.action_history ?? [], null, 2);
      await updateLiveView(session, obs);
    } catch (error) {
      observation.textContent = JSON.stringify({ error: String(error) }, null, 2);
      historyEl.textContent = '[]';
    }

    try {
      const dashboard = await json('/api/dashboard');
      tasksEl.textContent = JSON.stringify(dashboard.tasks ?? [], null, 2);
    } catch (error) {
      tasksEl.textContent = JSON.stringify({ error: String(error) }, null, 2);
    }
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
sessionPicker?.addEventListener('change', async () => {
  if (!sessionPicker.value) return;
  sessionId = sessionPicker.value;
  taskId = null;
  existingSessionInput.value = sessionId;
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
  refreshSessionPicker().catch(() => {});
}

setInterval(() => {
  refreshSessionPicker().catch(() => {});
  refresh().catch(() => {});
}, 3000);
