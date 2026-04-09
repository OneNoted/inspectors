import test from 'node:test';
import assert from 'node:assert/strict';
import { buildSessionUrl, getSessionIdFromLocation, parseSessionReference } from './session-link.js';

test('parseSessionReference accepts raw session ids', () => {
  assert.equal(parseSessionReference('40dcd2ac-a7e6-4b2d-8a61-edc2bc4323d4'), '40dcd2ac-a7e6-4b2d-8a61-edc2bc4323d4');
});

test('parseSessionReference extracts session ids from live-view urls', () => {
  assert.equal(
    parseSessionReference('http://127.0.0.1:3000/api/sessions/40dcd2ac-a7e6-4b2d-8a61-edc2bc4323d4/live-view/'),
    '40dcd2ac-a7e6-4b2d-8a61-edc2bc4323d4',
  );
});

test('getSessionIdFromLocation reads session query params', () => {
  assert.equal(getSessionIdFromLocation('?session=abc123'), 'abc123');
  assert.equal(getSessionIdFromLocation(''), null);
});

test('buildSessionUrl adds or removes the session query param', () => {
  const locationLike = { href: 'http://127.0.0.1:3000/?foo=bar' };
  assert.equal(buildSessionUrl('abc123', locationLike), '/?foo=bar&session=abc123');
  assert.equal(buildSessionUrl('', locationLike), '/?foo=bar');
});
