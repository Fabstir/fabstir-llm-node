// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * Per-directory write mutex for S5 bridge.
 *
 * Serializes writes to files sharing the same parent directory to prevent
 * S5.js "Revision number too low" retries under concurrent load.
 * Different directories run in parallel.
 */

/** @type {Map<string, Promise<void>>} */
const chains = new Map();

/**
 * Extract immediate parent directory from a file path.
 * @param {string} filePath
 * @returns {string} Parent directory, or '/' if no slash
 */
export function parentDir(filePath) {
  const lastSlash = filePath.lastIndexOf('/');
  return lastSlash < 0 ? '/' : filePath.slice(0, lastSlash);
}

/**
 * Acquire a directory lock. Concurrent calls with the same key are serialized;
 * different keys run in parallel.
 * @param {string} dirKey
 * @returns {Promise<() => void>} Release function — call in a finally block
 */
export async function acquireDirectoryLock(dirKey) {
  // Wait for the current chain tail (if any) before proceeding
  const prev = chains.get(dirKey) ?? Promise.resolve();

  let release;
  const next = new Promise((resolve) => {
    release = resolve;
  });

  // Append ourselves to the chain BEFORE awaiting, so later callers queue behind us
  chains.set(dirKey, next);

  await prev;

  return () => {
    // Clean up the key if we are still the tail (no one queued behind us)
    if (chains.get(dirKey) === next) {
      chains.delete(dirKey);
    }
    release();
  };
}

/**
 * Number of active directory lock keys (for health/diagnostics).
 * @returns {number}
 */
export function activeLockCount() {
  return chains.size;
}
