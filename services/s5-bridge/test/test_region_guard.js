// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
import { test } from 'node:test';
import assert from 'node:assert';
import { readFileSync } from 'node:fs';
import { REGIONS, regionErrors, liveAccountErrors, enforceUploadRegion } from '../src/region_guard.js';
import { bridgeConfig, validateConfig } from '../src/config.js';

const GLOBAL_PEER = 'wss://z2DcjTLqfj6PTMsDbFfgtuHtYmrKeibFTkvqY8QZeyR3YmE@s5.platformlessai.ai/s5/p2p';
const UK_PEER = 'wss://z2DXG8VjbxzY23kt6nJuYPriFEjQKHXJSpnWGSc7SPCgwvc@s5-uk.platformlessai.ai/s5/p2p';
const UK = { region: 'uk', portalUrl: 'https://s5-uk.platformlessai.ai', initialPeers: [UK_PEER] };
const GLOBAL = { region: 'global', portalUrl: 'https://s5.platformlessai.ai', initialPeers: [GLOBAL_PEER] };

// --- the table ---

test('the region table is exactly global and uk, each with its own portal and peer host', () => {
  assert.deepStrictEqual(Object.keys(REGIONS).sort(), ['global', 'uk']);
  assert.deepStrictEqual(REGIONS.global, { portalHost: 's5.platformlessai.ai', peerHost: 's5.platformlessai.ai' });
  assert.deepStrictEqual(REGIONS.uk, { portalHost: 's5-uk.platformlessai.ai', peerHost: 's5-uk.platformlessai.ai' });
});

test('a correct uk and a correct global configuration pass', () => {
  assert.deepStrictEqual(regionErrors(UK), []);
  assert.deepStrictEqual(regionErrors(GLOBAL), []);
});

// --- S5_REGION itself ---

test('S5_REGION missing or empty is refused: there is no default', () => {
  for (const region of [undefined, '', '   ']) {
    const e = regionErrors({ ...UK, region });
    assert.strictEqual(e.length, 1, `region=${JSON.stringify(region)}`);
    assert.match(e[0], /S5_REGION is required/);
  }
});

test('eu and unknown regions are refused, including a different case', () => {
  for (const region of ['eu', 'us', 'UK', 'Global', 'uk ']) {
    const e = regionErrors({ ...UK, region });
    if (region === 'uk ') {
      // surrounding whitespace is trimmed, not a different region
      assert.deepStrictEqual(e, []);
      continue;
    }
    assert.strictEqual(e.length, 1, region);
    assert.match(e[0], /S5_REGION/);
  }
  assert.match(regionErrors({ ...UK, region: 'eu' })[0], /not configured yet/);
});

test('a region name inherited from Object.prototype is not a region', () => {
  for (const region of ['constructor', 'toString', '__proto__']) {
    assert.strictEqual(regionErrors({ ...UK, region }).length, 1, region);
  }
});

// --- portal ---

test('a portal outside the region is refused: Global portal on a uk host and the reverse', () => {
  const e = regionErrors({ ...UK, portalUrl: 'https://s5.platformlessai.ai' });
  assert.strictEqual(e.length, 1);
  assert.match(e[0], /S5_PORTAL_URL host s5\.platformlessai\.ai is not the uk portal host s5-uk\.platformlessai\.ai/);
  assert.strictEqual(regionErrors({ ...GLOBAL, portalUrl: 'https://s5-uk.platformlessai.ai' }).length, 1);
});

test('a portal host that only contains the region host is refused', () => {
  for (const portalUrl of [
    'https://s5-uk.platformlessai.ai.example.com',
    'https://evil-s5-uk.platformlessai.ai',
    'https://s5-uk.platformlessai.ai@s5.platformlessai.ai',
  ]) {
    assert.strictEqual(regionErrors({ ...UK, portalUrl }).length, 1, portalUrl);
  }
});

test('a missing or unparseable portal URL is refused', () => {
  for (const portalUrl of [undefined, '', 'not a url', 's5-uk.platformlessai.ai']) {
    const e = regionErrors({ ...UK, portalUrl });
    assert.strictEqual(e.length, 1, String(portalUrl));
    assert.match(e[0], /S5_PORTAL_URL/);
  }
});

test('the portal may carry a port or path; the host is what is compared', () => {
  assert.deepStrictEqual(regionErrors({ ...UK, portalUrl: 'https://s5-uk.platformlessai.ai:443/' }), []);
  assert.deepStrictEqual(regionErrors({ ...UK, portalUrl: 'https://S5-UK.platformlessai.ai' }), []);
});

// --- peers ---

test('a Global peer on a uk host is refused, even beside the UK peer', () => {
  const e = regionErrors({ ...UK, initialPeers: [UK_PEER, GLOBAL_PEER] });
  assert.strictEqual(e.length, 1);
  assert.match(e[0], /S5_INITIAL_PEERS entry .* host s5\.platformlessai\.ai is not the uk peer host s5-uk\.platformlessai\.ai/);
  // the node id is not secret but the message names the host, which is what matters
  assert.strictEqual(regionErrors({ ...GLOBAL, initialPeers: [UK_PEER] }).length, 1);
});

test('a public S5 peer is refused', () => {
  const e = regionErrors({ ...UK, initialPeers: ['wss://z2Das8aEF7oNoxkcrfvzerZ1iBPWfm6D7gy3hVE4ALGSpVB@node.sfive.net/s5/p2p'] });
  assert.strictEqual(e.length, 1);
});

test('no peer at all is refused: a region host needs its own peer', () => {
  for (const initialPeers of [[], undefined, ['', '  ']]) {
    const e = regionErrors({ ...UK, initialPeers });
    assert.strictEqual(e.length, 1, JSON.stringify(initialPeers));
    assert.match(e[0], /at least one peer/);
  }
});

test('an unparseable peer is refused', () => {
  const e = regionErrors({ ...UK, initialPeers: [UK_PEER, 'garbage'] });
  assert.strictEqual(e.length, 1);
  assert.match(e[0], /cannot be parsed/);
});

test('every fault is reported together', () => {
  const e = regionErrors({ region: 'uk', portalUrl: 'https://s5.platformlessai.ai', initialPeers: [GLOBAL_PEER] });
  assert.strictEqual(e.length, 2);
});

// --- after login: the live account list and the pin ---

const UK_PORTAL = 'https://s5-uk.platformlessai.ai';

test('live accounts all on the region portal and pinned: no error', () => {
  const configs = { 's5-uk.platformlessai.ai:A': {}, 's5-uk.platformlessai.ai:B': {} };
  const pin = { pinned: true, host: 's5-uk.platformlessai.ai', before: 2, after: 2, dropped: [] };
  assert.deepStrictEqual(liveAccountErrors(configs, UK_PORTAL, pin), []);
});

test('a live account on another host is refused', () => {
  const configs = { 's5-uk.platformlessai.ai:A': {}, 's5.platformlessai.ai:B': {} };
  const pin = { pinned: true, host: 's5-uk.platformlessai.ai', before: 2, after: 2, dropped: [] };
  const e = liveAccountErrors(configs, UK_PORTAL, pin);
  assert.strictEqual(e.length, 1);
  assert.match(e[0], /s5\.platformlessai\.ai:B/);
});

test('a live account whose host merely starts with the region host is refused', () => {
  const configs = { 's5-uk.platformlessai.ai:A': {}, 's5-uk.platformlessai.ai.example:B': {} };
  const pin = { pinned: true, host: 's5-uk.platformlessai.ai', before: 2, after: 2, dropped: [] };
  assert.strictEqual(liveAccountErrors(configs, UK_PORTAL, pin).length, 1);
});

test('a pin that did not take is refused', () => {
  const pin = { pinned: false, host: 's5-uk.platformlessai.ai', before: 0, after: 0, dropped: [] };
  const e = liveAccountErrors({}, UK_PORTAL, pin);
  assert.ok(e.some((m) => /upload pin did not take/.test(m)), e.join('|'));
});

test('enforceUploadRegion pins first, then passes when only region accounts remain', () => {
  const api = { accountConfigs: { 's5.vup.cx:X': {}, 's5-uk.platformlessai.ai:A': {} } };
  const pin = enforceUploadRegion(api, UK_PORTAL);
  assert.strictEqual(pin.pinned, true);
  assert.deepStrictEqual(Object.keys(api.accountConfigs), ['s5-uk.platformlessai.ai:A']);
});

test('enforceUploadRegion throws when no account is on the region portal', () => {
  const api = { accountConfigs: { 's5.platformlessai.ai:B': {} } };
  assert.throws(() => enforceUploadRegion(api, UK_PORTAL), /upload pin did not take/);
  assert.throws(() => enforceUploadRegion({ accountConfigs: {} }, UK_PORTAL), /upload pin did not take/);
  assert.throws(() => enforceUploadRegion(undefined, UK_PORTAL), /upload pin did not take/);
});

// --- wiring ---

function withConfig(patch, fn) {
  const saved = { ...bridgeConfig };
  Object.assign(bridgeConfig, patch);
  try { return fn(); } finally { Object.assign(bridgeConfig, saved); }
}

test('validateConfig refuses a missing S5_REGION and a wrong portal, accepts a correct uk set', () => {
  const ok = { seedPhrase: 'x', port: 5522, region: 'uk', portalUrl: UK.portalUrl, initialPeers: [UK_PEER] };
  withConfig(ok, () => assert.doesNotThrow(() => validateConfig()));
  withConfig({ ...ok, region: '' }, () => assert.throws(() => validateConfig(), /S5_REGION is required/));
  withConfig({ ...ok, portalUrl: 'https://s5.platformlessai.ai' }, () => assert.throws(() => validateConfig(), /S5_PORTAL_URL/));
  withConfig({ ...ok, initialPeers: [GLOBAL_PEER] }, () => assert.throws(() => validateConfig(), /S5_INITIAL_PEERS/));
});

test('the static guard runs before the S5 client starts, the live guard before the filesystem', () => {
  const server = readFileSync(new URL('../src/server.js', import.meta.url), 'utf8');
  const v = server.indexOf('validateConfig();');
  const i = server.indexOf('await initializeS5Client();');
  assert.ok(v > 0 && i > v, 'validateConfig() must run before initializeS5Client()');

  const client = readFileSync(new URL('../src/s5_client.js', import.meta.url), 'utf8');
  const g = client.indexOf('enforceUploadRegion(s5.apiWithIdentity, bridgeConfig.portalUrl)');
  const f = client.indexOf('await s5.fs.ensureIdentityInitialized()');
  assert.ok(g > 0, 'initializeS5Client must call enforceUploadRegion');
  assert.ok(f > g, 'enforceUploadRegion must run before the filesystem is initialised');
  assert.ok(!/pinUploadPortal\(/.test(client), 's5_client must pin only through enforceUploadRegion');
});
