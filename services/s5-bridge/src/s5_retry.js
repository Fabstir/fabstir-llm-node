// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * Retry an S5 filesystem call across a TRANSIENT directory failure.
 *
 * Since @julesl23/s5js 0.9.0-beta.50 a directory blob that the registry names
 * but the portal cannot serve yet throws a retryable S5DirectoryLoadError
 * instead of reading as empty (see s5_read_errors.js). On the upload path this
 * is read-after-write: fs.put() rewrites the parent directory, and the next
 * read of it (pathToCID() here, or the next file's fs.put() into the same
 * directory) can see the new blob as 404 for a few seconds. Nothing retried
 * it, so one blip failed the whole upload with a 500, and the node ends a
 * training run on the first failed shard (2026-09-28 rehearsal, job 1168,
 * adapter_model.safetensors.shard10).
 *
 * Only the retryable class is retried. A structural error (the directory
 * needs fs.repairDirectory()) and every other error are rethrown at once.
 */
import { isS5DirectoryLoadError } from '@julesl23/s5js';

export const DIR_RETRY_ATTEMPTS = 8;
export const DIR_RETRY_BASE_MS = 2000; // linear: 2+4+...+14 s = 56 s worst case

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * @template T
 * @param {() => Promise<T>} fn
 * @param {{attempts?: number, baseMs?: number,
 *          onRetry?: (info: {attempt: number, delayMs: number, error: any}) => void,
 *          wait?: (ms: number) => Promise<void>}} [opts]
 * @returns {Promise<T>}
 */
export async function withDirectoryRetry(fn, opts = {}) {
  const {
    attempts = DIR_RETRY_ATTEMPTS,
    baseMs = DIR_RETRY_BASE_MS,
    onRetry = () => {},
    wait = sleep,
  } = opts;
  for (let attempt = 1; ; attempt++) {
    try {
      return await fn();
    } catch (error) {
      const transient = isS5DirectoryLoadError(error) && error.retryable === true;
      if (!transient || attempt >= attempts) throw error;
      const delayMs = baseMs * attempt;
      onRetry({ attempt, delayMs, error });
      await wait(delayMs);
    }
  }
}
