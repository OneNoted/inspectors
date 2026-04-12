export function parseSessionReference(input) {
  const trimmed = String(input ?? '').trim();
  if (!trimmed) return null;

  try {
    const url = new URL(trimmed);
    const fromQuery = url.searchParams.get('session');
    if (fromQuery) return fromQuery;
    const match = url.pathname.match(/\/api\/sessions\/([^/]+)/);
    if (match) return match[1];
  } catch {
    // Fall through to raw-string parsing.
  }

  const queryMatch = trimmed.match(/[?&]session=([^&]+)/);
  if (queryMatch) return decodeURIComponent(queryMatch[1]);

  const pathMatch = trimmed.match(/\/api\/sessions\/([^/]+)/);
  if (pathMatch) return pathMatch[1];

  return trimmed;
}

export function getSessionIdFromLocation(search = window.location.search) {
  const params = new URLSearchParams(search);
  return params.get('session');
}

export function buildSessionUrl(sessionId, locationLike = window.location) {
  const url = new URL(locationLike.href);
  if (sessionId) {
    url.searchParams.set('session', sessionId);
  } else {
    url.searchParams.delete('session');
  }
  return `${url.pathname}${url.search}`;
}
