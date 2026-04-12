// ui-enhancements.js
// ------------------
// Layered on top of app.js without modifying it. Provides:
//  1. Status-pill state + trust-stripe data attribute from badge + trust text
//  2. Top-bar session chip rendering from #session-summary
//  3. QEMU options slide-open transition mirrored from the `hidden` attribute
//  4. <pre> scroll position + selection preservation across app.js's 3s poll
//  5. "updated Ns ago" ticker with pulse on new desktop frames
//  6. Keyboard shortcuts (r / c / esc / ?) and the cheatsheet dialog
//
// Every subsystem is additive and idempotent. Nothing here mutates state that
// app.js owns; it only reads app.js's output and augments the visuals.

/* ---------------------------------------------------------------------
   1. Badge state + trust stripe
   --------------------------------------------------------------------- */

const badge = document.getElementById('live-view-badge');
const trustText = document.getElementById('live-view-trust');
const pillWrap = badge?.closest('.pill-wrap') ?? null;
const desktopStage = document.querySelector('.desktop-stage');

function derivePillState(text) {
  const t = (text || '').toLowerCase();
  if (t.includes('unavailable')) return 'unavailable';
  if (t.includes('screenshot')) return 'screenshot';
  if (t.includes('live')) return 'live';
  return 'unavailable';
}

function deriveTrust(badgeText, trustString) {
  const b = (badgeText || '').toLowerCase();
  const tr = (trustString || '').toLowerCase();
  if (b.includes('unavailable')) return 'unavailable';
  const diverged = tr.includes('does not match');
  if (b.includes('live')) return diverged ? 'fallback' : 'live';
  if (b.includes('screenshot')) return diverged ? 'fallback' : 'screenshot';
  return 'unknown';
}

function syncBadgeState() {
  if (!badge) return;
  const pillState = derivePillState(badge.textContent);
  const trust = deriveTrust(badge.textContent, trustText?.textContent);
  if (pillWrap) pillWrap.setAttribute('data-state', pillState);
  if (desktopStage) desktopStage.setAttribute('data-trust', trust);
}

if (badge) {
  new MutationObserver(syncBadgeState).observe(badge, {
    childList: true,
    characterData: true,
    subtree: true,
  });
}
if (trustText) {
  new MutationObserver(syncBadgeState).observe(trustText, {
    childList: true,
    characterData: true,
    subtree: true,
  });
}
syncBadgeState();

/* ---------------------------------------------------------------------
   2. Session chips (split `provider=x · ready=ok` into styled chips)
   --------------------------------------------------------------------- */

const sessionSummary = document.getElementById('session-summary');
const chipContainer = document.querySelector('.session-chips');

function classifyChipValue(key, value) {
  const v = (value || '').toLowerCase();
  if (!v || v === 'n/a' || v === 'none') return null;
  if (v.includes('unavailable') || v.includes('error') || v.includes('failed')) return 'err';
  if (v.includes('fallback') || v.includes('pending') || v.includes('degraded')) return 'warn';
  if (['ok', 'ready', 'connected', 'running', 'live'].some((s) => v === s || v.endsWith('/' + s))) {
    return 'ok';
  }
  return null;
}

function renderChips() {
  if (!chipContainer || !sessionSummary) return;
  const text = (sessionSummary.textContent || '').trim();
  chipContainer.replaceChildren();
  if (!text || text === 'No session' || text.toLowerCase().startsWith('enter a session')) {
    return;
  }
  const parts = text.split(' · ');
  for (const part of parts) {
    const eq = part.indexOf('=');
    if (eq === -1) continue;
    const key = part.slice(0, eq);
    const value = part.slice(eq + 1);
    const chip = document.createElement('span');
    chip.className = 'chip';
    const state = classifyChipValue(key, value);
    if (state) chip.setAttribute('data-state', state);
    const k = document.createElement('span');
    k.className = 'chip-key';
    k.textContent = key;
    const v = document.createElement('span');
    v.className = 'chip-val';
    v.textContent = value;
    chip.append(k, v);
    chipContainer.append(chip);
  }
}

if (sessionSummary) {
  new MutationObserver(renderChips).observe(sessionSummary, {
    childList: true,
    characterData: true,
    subtree: true,
  });
}
renderChips();

/* ---------------------------------------------------------------------
   3. QEMU options slide-open mirror
   --------------------------------------------------------------------- */

const qemuOptions = document.getElementById('qemu-options');
const qemuCollapse = qemuOptions?.closest('.collapse') ?? null;

function syncQemuCollapse() {
  if (!qemuOptions || !qemuCollapse) return;
  qemuCollapse.classList.toggle('open', !qemuOptions.hasAttribute('hidden'));
}

if (qemuOptions && qemuCollapse) {
  new MutationObserver(syncQemuCollapse).observe(qemuOptions, {
    attributes: true,
    attributeFilter: ['hidden'],
  });
  syncQemuCollapse();
}

/* ---------------------------------------------------------------------
   4. <pre> scroll + selection preservation across app.js's 3s poll
   --------------------------------------------------------------------- */

const preservePres = ['session-meta', 'observation', 'history', 'tasks']
  .map((id) => document.getElementById(id))
  .filter(Boolean);

for (const pre of preservePres) {
  let savedScroll = 0;
  let snapshot = pre.textContent;
  let selectionLocked = false;
  let reverting = false;

  pre.addEventListener('scroll', () => {
    savedScroll = pre.scrollTop;
  });

  document.addEventListener('selectionchange', () => {
    const sel = document.getSelection();
    const inside = sel && sel.rangeCount > 0 && sel.containsNode && sel.containsNode(pre, true);
    if (inside) {
      selectionLocked = true;
      snapshot = pre.textContent;
    } else if (selectionLocked) {
      selectionLocked = false;
      snapshot = pre.textContent;
    }
  });

  const observer = new MutationObserver(() => {
    if (reverting) return;
    if (selectionLocked && pre.textContent !== snapshot) {
      // User has an active selection — revert this poll's update to avoid
      // clobbering what they're reading. Next poll will land once the
      // selection clears.
      reverting = true;
      pre.textContent = snapshot;
      // Give the browser a microtask to settle before unsetting the guard.
      queueMicrotask(() => { reverting = false; });
      return;
    }
    // Not locked — absorb the new content as the new snapshot and restore
    // scroll position so the operator's viewport doesn't jump.
    snapshot = pre.textContent;
    if (savedScroll) pre.scrollTop = savedScroll;
  });

  observer.observe(pre, {
    childList: true,
    characterData: true,
    subtree: true,
  });
}

/* ---------------------------------------------------------------------
   5. "updated Ns ago" ticker + desktop-stage pulse on new frames
   --------------------------------------------------------------------- */

const desktopImage = document.getElementById('desktop-image');
const viewerFrame = document.getElementById('viewer-frame');
const tickerNode = document.querySelector('.ticker');
const tickerTime = tickerNode?.querySelector('.ticker-time') ?? null;

let lastFrameAt = null;
let pulseTimer = null;

function markFresh() {
  lastFrameAt = Date.now();
  if (tickerNode) tickerNode.setAttribute('data-fresh', 'true');
  if (desktopStage) {
    desktopStage.classList.remove('pulse');
    // Force reflow so the animation restarts cleanly.
    void desktopStage.offsetWidth;
    desktopStage.classList.add('pulse');
  }
  if (pulseTimer) clearTimeout(pulseTimer);
  pulseTimer = setTimeout(() => {
    desktopStage?.classList.remove('pulse');
  }, 460);
}

function formatAge(ms) {
  if (ms == null) return '—';
  const s = ms / 1000;
  if (s < 1) return 'just now';
  if (s < 10) return `${s.toFixed(1)}s ago`;
  if (s < 60) return `${Math.round(s)}s ago`;
  const m = Math.floor(s / 60);
  const rem = Math.round(s % 60);
  return `${m}m ${rem}s ago`;
}

function tickTicker() {
  if (!tickerTime) return;
  if (lastFrameAt == null) {
    tickerTime.textContent = '—';
    tickerNode?.setAttribute('data-fresh', 'false');
    return;
  }
  const age = Date.now() - lastFrameAt;
  tickerTime.textContent = formatAge(age);
  if (age > 5000) tickerNode?.setAttribute('data-fresh', 'false');
}

setInterval(tickTicker, 500);

if (desktopImage) {
  desktopImage.addEventListener('load', markFresh);
}
if (viewerFrame) {
  new MutationObserver(() => {
    if (viewerFrame.hasAttribute('src')) markFresh();
  }).observe(viewerFrame, { attributes: true, attributeFilter: ['src'] });
}

/* ---------------------------------------------------------------------
   6. Keyboard shortcuts + cheatsheet dialog
   --------------------------------------------------------------------- */

const shortcutSheet = document.getElementById('shortcut-sheet');
const shortcutOpen = document.getElementById('shortcut-open');

function openSheet() {
  if (!shortcutSheet || shortcutSheet.open) return;
  if (typeof shortcutSheet.showModal === 'function') {
    shortcutSheet.showModal();
  } else {
    shortcutSheet.setAttribute('open', '');
  }
}

function closeSheet() {
  if (!shortcutSheet || !shortcutSheet.open) return;
  if (typeof shortcutSheet.close === 'function') {
    shortcutSheet.close();
  } else {
    shortcutSheet.removeAttribute('open');
  }
}

function toggleSheet() {
  if (!shortcutSheet) return;
  if (shortcutSheet.open) closeSheet(); else openSheet();
}

shortcutOpen?.addEventListener('click', toggleSheet);

shortcutSheet?.addEventListener('click', (event) => {
  // Dismiss on backdrop click (target is the dialog itself, not inner content).
  if (event.target === shortcutSheet) closeSheet();
});

function isTypingTarget(el) {
  if (!el) return false;
  const tag = el.tagName;
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true;
  if (el.isContentEditable) return true;
  return false;
}

document.addEventListener('keydown', (event) => {
  // Shortcut sheet toggle works even inside inputs for `?` + shift,
  // but other shortcuts bail when typing.
  if (event.key === '?' || (event.key === '/' && event.shiftKey)) {
    if (isTypingTarget(document.activeElement)) return;
    event.preventDefault();
    toggleSheet();
    return;
  }

  if (event.key === 'Escape') {
    if (shortcutSheet?.open) {
      // Native <dialog> handles Esc itself, but be explicit.
      event.preventDefault();
      closeSheet();
      return;
    }
    if (isTypingTarget(document.activeElement)) return;
    event.preventDefault();
    document.getElementById('clear-session')?.click();
    return;
  }

  if (isTypingTarget(document.activeElement)) return;
  if (event.metaKey || event.ctrlKey || event.altKey) return;

  if (event.key === 'r' || event.key === 'R') {
    event.preventDefault();
    document.getElementById('refresh-session')?.click();
    return;
  }

  if (event.key === 'c' || event.key === 'C') {
    event.preventDefault();
    document.getElementById('task-description')?.focus();
    return;
  }
});
