export function getLiveDesktopView(session) {
  return session?.live_desktop_view ?? {
    mode: 'unavailable',
    status: 'unavailable',
    provider_surface: 'none',
    matches_action_plane: false,
    canonical_url: null,
    debug_url: session?.viewer_url ?? null,
    reason: 'live desktop metadata unavailable',
    refresh_interval_ms: null,
  };
}

function getObservationActiveWindow(observation) {
  if (!observation) return null;
  if (typeof observation.active_window === 'string' && observation.active_window) {
    return observation.active_window;
  }
  if (observation.active_window?.title) {
    return observation.active_window.title;
  }
  if (typeof observation.summary?.active_window === 'string' && observation.summary.active_window) {
    return observation.summary.active_window;
  }
  return null;
}

function hasUnverifiedBrowserFallback(observation) {
  return (observation?.action_history ?? []).some((entry) =>
    entry?.action?.kind === 'browser_open' && entry?.source === 'browser-open-fallback');
}

export function describeLiveDesktopView(session, observation = null) {
  const liveView = getLiveDesktopView(session);
  const activeWindow = getObservationActiveWindow(observation);
  const isIdleXvfbFallback = session?.provider === 'xvfb' && !activeWindow;

  if (liveView.mode === 'stream' && liveView.status === 'ready' && liveView.canonical_url) {
    return {
      title: 'Live desktop view',
      badge: 'Live desktop',
      trustText: liveView.matches_action_plane
        ? 'The embedded stream matches the session action plane.'
        : 'The embedded stream is visible, but it does not match the action plane.',
      showFrame: true,
      showImage: false,
      showPlaceholder: false,
      placeholderText: '',
      debugLinkLabel: liveView.debug_url ? 'Open raw viewer' : null,
    };
  }

  if (liveView.mode === 'screenshot_poll' && liveView.status === 'ready' && liveView.canonical_url) {
    if (isIdleXvfbFallback) {
      const browserFallbackAttempted = hasUnverifiedBrowserFallback(observation);
      return {
        title: 'Desktop screenshot fallback',
        badge: browserFallbackAttempted ? 'Fallback idle' : 'Fallback ready',
        trustText: browserFallbackAttempted
          ? 'Xvfb accepted the browser-open fallback, but no visible window appeared. Use QEMU product for a trustworthy browser view.'
          : 'Xvfb fallback is running, but no visible window is open yet. Use QEMU product when you need a trustworthy live app view.',
        showFrame: false,
        showImage: false,
        showPlaceholder: true,
        placeholderText: browserFallbackAttempted
          ? 'No visible Xvfb window appeared after browser_open'
          : 'No visible Xvfb window is currently open',
        debugLinkLabel: liveView.debug_url ? 'Open debug VM viewer' : null,
      };
    }

    return {
      title: 'Desktop screenshot fallback',
      badge: 'Screenshot fallback',
      trustText: liveView.matches_action_plane
        ? 'Showing the action plane via screenshot polling.'
        : 'Showing screenshot polling only; this does not match the action plane.',
      showFrame: false,
      showImage: true,
      showPlaceholder: false,
      placeholderText: '',
      debugLinkLabel: liveView.debug_url ? 'Open debug VM viewer' : null,
    };
  }

  return {
    title: liveView.mode === 'screenshot_poll' ? 'Desktop screenshot fallback' : 'Live desktop view',
    badge: 'Unavailable',
    trustText: liveView.reason ?? 'No trustworthy live desktop surface is currently available.',
    showFrame: false,
    showImage: false,
    showPlaceholder: true,
    placeholderText: liveView.mode === 'screenshot_poll'
      ? 'Screenshot fallback unavailable'
      : 'Live desktop unavailable',
    debugLinkLabel: liveView.debug_url ? 'Open debug VM viewer' : null,
  };
}

export function buildScreenshotUrl(liveView) {
  if (!liveView?.canonical_url) return null;
  return `${liveView.canonical_url}?ts=${Date.now()}`;
}
