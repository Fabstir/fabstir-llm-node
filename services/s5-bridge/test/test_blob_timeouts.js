// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * Large S5 blob downloads get longer waits than s5.js's defaults (run 2, 2026-09-30: a
 * trained adapter could not be served back because 24 MiB shards took 2.5 to 3.4 s to
 * start and s5.js allows 3 s).
 */
import { test } from 'node:test';
import assert from 'node:assert';
import { applyBlobTimeouts, DEFAULT_BLOB_HEADERS_TIMEOUT_MS, DEFAULT_BLOB_DISCOVERY_TIMEOUT_MS } from '../src/blob_timeouts.js';

function fakeNode() {
  const calls = [];
  return {
    calls,
    // s5.js's signature and defaults (0.9.0-beta.55 node/node.js)
    async downloadBlobAsBytes(hash, timeoutMs = 10000, headersTimeoutMs = 3000, bodyTimeoutMs = 60000, exhaustedGraceMs = 1000) {
      calls.push({ self: this, hash, timeoutMs, headersTimeoutMs, bodyTimeoutMs, exhaustedGraceMs });
      return new Uint8Array([1]);
    },
  };
}

test('a hash-only call (how S5APIWithIdentity calls it) gets the longer waits', async () => {
  const node = fakeNode();
  applyBlobTimeouts(node);
  await node.downloadBlobAsBytes('h');
  const c = node.calls[0];
  assert.strictEqual(c.headersTimeoutMs, DEFAULT_BLOB_HEADERS_TIMEOUT_MS);
  assert.strictEqual(c.timeoutMs, DEFAULT_BLOB_DISCOVERY_TIMEOUT_MS);
  assert.ok(c.headersTimeoutMs > 3000, 'the 3 s s5.js default is the defect');
  assert.strictEqual(c.bodyTimeoutMs, 60000, 'body timeout left to s5.js');
  assert.strictEqual(c.self, node, 'still called as a method of the node');
});

test('configured values win, and an explicit caller value is kept', async () => {
  const node = fakeNode();
  applyBlobTimeouts(node, { headersTimeoutMs: 12345, discoveryTimeoutMs: 54321 });
  await node.downloadBlobAsBytes('h');
  await node.downloadBlobAsBytes('h', 1, 2);
  assert.deepStrictEqual([node.calls[0].timeoutMs, node.calls[0].headersTimeoutMs], [54321, 12345]);
  assert.deepStrictEqual([node.calls[1].timeoutMs, node.calls[1].headersTimeoutMs], [1, 2]);
});

test('the bridge wires it in right after S5.create', async () => {
  const { readFileSync } = await import('node:fs');
  const src = readFileSync(new URL('../src/s5_client.js', import.meta.url), 'utf8');
  assert.match(src, /applyBlobTimeouts\(s5\.node,/);
});
