// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * How a failed S5 read is reported to a caller.
 *
 * This is a one-rule module because the rule is the thing that broke: callers
 * (the indexer, the node, the transcoder) treat 404 as "this does not exist"
 * and write that down as fact. A transient S5 directory failure reported as 404
 * is therefore not a slow read — it is a permanently empty storefront, recorded
 * from a degraded pass. The library cannot enforce this for us; since
 * @julesl23/s5js 0.9.0-beta.50 it can only tell us which case we are in.
 *
 * absent      → 404  the registry has no entry. Definitive. Safe to record.
 * retryable   → 503  the directory is known but unreadable right now. Retry.
 * structural  → 502  the directory is broken. Needs fs.repairDirectory(), not a retry.
 */
import { isS5DirectoryLoadError } from '@julesl23/s5js';

/**
 * @param {unknown} error  the thrown value from an s5.fs read
 * @param {string}  path   the path the caller asked for
 * @returns {{status: number, headers: Record<string,string>, body: object}}
 */
export function classifyS5ReadError(error, path) {
  if (isS5DirectoryLoadError(error)) {
    if (error.retryable) {
      return {
        status: 503,
        headers: { 'Retry-After': '2' },
        body: {
          error: 'S5 directory temporarily unavailable',
          path,
          // The directory that actually failed, which is what repairDirectory()
          // takes — not necessarily the path that was asked for.
          failedPath: error.path,
          publicKey: error.publicKey,
          retryable: true,
          message: error.message,
        },
      };
    }
    return {
      status: 502,
      headers: {},
      body: {
        error: 'S5 directory is structurally broken and needs repair',
        path,
        failedPath: error.path,
        publicKey: error.publicKey,
        retryable: false,
        message: error.message,
      },
    };
  }

  return {
    status: 404,
    headers: {},
    body: {
      error: 'File not found or download failed',
      path,
      message: error?.message ?? String(error),
    },
  };
}
