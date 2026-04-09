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

export function describeLiveDesktopView(session) {
  const liveView = getLiveDesktopView(session);

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

export function buildScreenshotUrl(sessionId, liveView) {
  if (!sessionId || !liveView?.canonical_url) return null;
  return `${liveView.canonical_url}?ts=${Date.now()}`;
}
