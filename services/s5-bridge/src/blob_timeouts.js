// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * Longer waits for S5 blob DOWNLOADS than s5.js's defaults.
 *
 * @julesl23/s5js 0.9.0-beta.55 `S5Node.downloadBlobAsBytes(hash, timeoutMs = 10000,
 * headersTimeoutMs = 3000, …)` gives each storage location 3 s to send response headers.
 * A 24 MiB blob takes 2.5 to 3.4 s to start from the storage we use when fetched from the
 * US (Phase 5 run 2, 2026-09-30: adapter shards 0 to 8 at 2.5 to 3.4 s, shard 9 over 3 s),
 * so large downloads fail at random and a trained adapter cannot be served back. The
 * S5APIWithIdentity calls `node.downloadBlobAsBytes(hash)` with the hash only, so wrapping
 * the node's method to supply larger defaults changes every download path at once, and a
 * caller that passes its own values keeps them.
 */
export const DEFAULT_BLOB_HEADERS_TIMEOUT_MS = 20000;
export const DEFAULT_BLOB_DISCOVERY_TIMEOUT_MS = 45000;

export function applyBlobTimeouts(node, {
  headersTimeoutMs = DEFAULT_BLOB_HEADERS_TIMEOUT_MS,
  discoveryTimeoutMs = DEFAULT_BLOB_DISCOVERY_TIMEOUT_MS,
} = {}) {
  if (!node || typeof node.downloadBlobAsBytes !== 'function') {
    throw new Error('applyBlobTimeouts: node has no downloadBlobAsBytes');
  }
  const original = node.downloadBlobAsBytes.bind(node);
  node.downloadBlobAsBytes = (hash, timeoutMs = discoveryTimeoutMs,
    headers = headersTimeoutMs, bodyTimeoutMs, exhaustedGraceMs) =>
    original(hash, timeoutMs, headers, bodyTimeoutMs, exhaustedGraceMs);
  return { headersTimeoutMs, discoveryTimeoutMs };
}
