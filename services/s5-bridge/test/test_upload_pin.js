// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
import { test } from 'node:test';
import assert from 'node:assert';
import { pinUploadPortal } from '../src/upload_pin.js';

const OURS = 'https://s5.platformlessai.ai';

test('drops every account not on our portal, keeps ours in order', () => {
  // host2's identity on 16 Nov: the public portal first, then ours.
  const configs = {
    's5.vup.cx:SbYNqwJggexXzHNt': { p: 'vup' },
    's5.platformlessai.ai:LnwIK4sjRbmiz6fk': { p: 'a' },
    's5.platformlessai.ai:MjjOM9fc1gYkDqZd': { p: 'b' },
    's5.garden:x': { p: 'garden' },
  };
  const r = pinUploadPortal(configs, OURS);
  assert.deepStrictEqual(r, {
    pinned: true, host: 's5.platformlessai.ai', before: 4, after: 2,
    dropped: ['s5.vup.cx:SbYNqwJggexXzHNt', 's5.garden:x'],
  });
  assert.deepStrictEqual(Object.keys(configs), ['s5.platformlessai.ai:LnwIK4sjRbmiz6fk', 's5.platformlessai.ai:MjjOM9fc1gYkDqZd']);
  // What the uploaders iterate (tus: Object.values) holds only ours.
  assert.deepStrictEqual(Object.values(configs).map((c) => c.p), ['a', 'b']);
});

test('a host that only contains ours as a substring is not ours', () => {
  const configs = { 'evil-s5.platformlessai.ai.example:1': {}, 's5.platformlessai.ai:2': {} };
  pinUploadPortal(configs, OURS);
  assert.deepStrictEqual(Object.keys(configs), ['s5.platformlessai.ai:2']);
});

test('no account on our portal: nothing dropped, not pinned', () => {
  const configs = { 's5.vup.cx:1': {} };
  assert.deepStrictEqual(pinUploadPortal(configs, OURS), { pinned: false, host: 's5.platformlessai.ai', before: 1, after: 1, dropped: [] });
  assert.deepStrictEqual(Object.keys(configs), ['s5.vup.cx:1']);
});

test('only ours already: unchanged; no configs at all: not pinned', () => {
  const configs = { 's5.platformlessai.ai:1': {} };
  assert.strictEqual(pinUploadPortal(configs, OURS).dropped.length, 0);
  assert.deepStrictEqual(Object.keys(configs), ['s5.platformlessai.ai:1']);
  assert.strictEqual(pinUploadPortal(undefined, OURS).pinned, false);
});

test('the portal URL may carry a port or path', () => {
  const configs = { 'portal.local:8443:1': {}, 'other:2': {} };
  pinUploadPortal(configs, 'https://portal.local:8443/s5');
  assert.deepStrictEqual(Object.keys(configs), ['portal.local:8443:1']);
});
