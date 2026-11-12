// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * Retention: delete a host's old encrypted job results and RELEASE their data on the portal.
 *
 * What s5.js does by itself (read in the bundled 0.9.0-beta.55, dist/src):
 *   - `fs.delete(path)` (fs/fs5.js) removes the ENTRY from its parent directory, and removes a directory only
 *     when it is empty. It never tells the portal anything about the blob the entry pointed to.
 *   - `unpinHash(hash)` (identity/api.js) is a stub that returns at once.
 * So an entry deleted through s5.js alone leaves the blob pinned to this host's account on the portal. Releasing
 * it takes one call of the portal's accounts API per blob, made here:
 *
 *     DELETE {portal}/s5/delete/{cid}      Authorization: the account's token
 *
 * with the cid in the stock S5 node's own form (see `portalCid`). That route and that cid form are how the stock
 * S5 node drops an account's pin; they are NOT exercised by anything in this repository's tests beyond a mock.
 * Whether the live portal answers them, and whether it then removes the bytes from its store, can only be shown
 * against the live portal.
 *
 * Order matters: the blob is released FIRST, while the entry still holds its hash. If the portal refuses, the
 * entry stays and the next run tries again; an entry is never removed while its blob is still pinned, because
 * the hash would be lost with it.
 */
import { Buffer } from 'node:buffer';

/** The only trees this bridge will prune, whatever the caller asks for. */
export const PRUNABLE_PREFIXES = ['home/ltx'];
/** One job's folder, or its attestation file beside it. */
const JOB_DIR = /^job_[A-Za-z0-9_-]{1,80}$/;
const JOB_FILE = /^job_[A-Za-z0-9_-]{1,80}_attestation\.json$/;
export const MAX_ENTRIES_PER_RUN = 500;
const PORTAL_TIMEOUT_MS = 20_000;

/**
 * The cid the portal's accounts API is given: the stock S5 node's raw-file CID, multibase base64url (`u`):
 * 0x26, the blake3 multihash with the type byte 0x1f the stock node stores under (see tus_upload.js), then the
 * size, little-endian with trailing zero bytes dropped.
 */
export function portalCid(hash, size) {
  const digest = hash.length === 33 ? hash.subarray(1) : hash;
  if (digest.length !== 32) {
    throw new Error('not a blake3 hash');
  }
  let n = BigInt(size);
  const sizeBytes = [];
  do {
    sizeBytes.push(Number(n & 0xffn));
    n >>= 8n;
  } while (n > 0n);
  return 'u' + Buffer.from([0x26, 0x1f, ...digest, ...sizeBytes]).toString('base64url');
}

/**
 * Drop this account's pin on one blob, on every configured portal. Resolves true when every portal answered
 * 2xx or 404 (already gone); false otherwise (or when no portal is configured: nothing was released).
 */
export async function releaseBlob(api, hash, size, opts = {}) {
  const fetchImpl = opts.fetchImpl ?? fetch;
  const portals = Object.values(api?.accountConfigs ?? {});
  if (portals.length === 0) {
    if (typeof opts.onFail === 'function') opts.onFail('no-portal-account');
    return false;
  }
  const why = typeof opts.onFail === 'function' ? opts.onFail : () => {};
  const cid = portalCid(hash, size);
  for (const portal of portals) {
    const auth = portal.headers?.['Authorization'] || portal.headers?.['authorization'] || '';
    try {
      const res = await fetchImpl(portal.apiURL(`delete/${cid}`), {
        method: 'DELETE',
        headers: { Authorization: auth },
        signal: AbortSignal.timeout(opts.timeoutMs ?? PORTAL_TIMEOUT_MS),
      });
      if (!(res.ok || res.status === 404)) {
        // the portal's own error words (never a path or a hash), reduced to a short slug
        let said = '';
        try {
          said = (await res.text()).slice(0, 200).replace(/[^A-Za-z ]+/g, ' ').trim().split(/\s+/).slice(0, 6).join('-').toLowerCase();
        } catch {
          said = '';
        }
        why(`release-http-${res.status}${said ? `:${said}` : ''}`);
        return false;
      }
    } catch (e) {
      why(`release-${e?.name === 'TimeoutError' ? 'timeout' : 'error'}`);
      return false;
    }
  }
  return true;
}

/**
 * A listing's recorded time in seconds. s5.js (fs/fs5.js `list`) gives milliseconds for a plain directory and an
 * ISO string for a sharded one; anything else is "unknown" (NaN).
 */
export function recordedSecs(timestamp) {
  if (typeof timestamp === 'number' && Number.isFinite(timestamp) && timestamp > 0) {
    return Math.floor(timestamp > 1e11 ? timestamp / 1000 : timestamp);
  }
  if (typeof timestamp === 'string') {
    const ms = Date.parse(timestamp);
    return Number.isFinite(ms) && ms > 0 ? Math.floor(ms / 1000) : NaN;
  }
  return NaN;
}

async function children(fs, path) {
  const out = [];
  for await (const item of fs.list(path)) {
    out.push(item);
  }
  return out;
}

/**
 * Delete the job entries under `prefix` whose recorded time is older than `olderThanSecs`.
 *
 * A listing (s5.js `fs.list`) gives each entry's name, type, size and recorded time, not its hash; the hash comes
 * from the advanced client (`pathToCID`, the file reference's blake3 hash), passed as `opts.advanced`.
 *
 * A job folder's time is the NEWEST time recorded on any file in it; an entry with no recorded time, or with
 * anything in it that is not a plain file, is left alone. Names in `exclude` (jobs the node is working on) are
 * never touched. At most `maxEntries` entries are deleted per call. Returns counts only.
 */
export async function pruneOlderThan(s5, params, opts = {}) {
  const { prefix, olderThanSecs } = params;
  if (!PRUNABLE_PREFIXES.includes(prefix)) {
    throw new RangeError('prefix is not prunable');
  }
  if (!Number.isInteger(olderThanSecs) || olderThanSecs < 86_400) {
    throw new RangeError('olderThanSecs must be a whole number of seconds, at least one day');
  }
  const maxEntries = Math.min(
    Number.isInteger(params.maxEntries) && params.maxEntries > 0 ? params.maxEntries : MAX_ENTRIES_PER_RUN,
    MAX_ENTRIES_PER_RUN,
  );
  const exclude = new Set(Array.isArray(params.exclude) ? params.exclude.map(String) : []);
  const now = opts.nowSecs ?? Math.floor(Date.now() / 1000);
  const cutoff = now - olderThanSecs;
  const lock = opts.lock ?? (async () => () => {});
  const fs = s5.fs;
  const api = s5.apiWithIdentity;
  const advanced = opts.advanced;
  if (!advanced?.pathToCID) {
    throw new Error('advanced client not available');
  }
  // `why` tallies the reason of each failed step by kind (never a path, a hash or any content), so a run that
  // deletes nothing can be diagnosed from the log line alone.
  const counts = { scanned: 0, deleted: 0, files: 0, released: 0, failed: 0, skipped: 0, more: false, why: {} };
  const note = (kind) => {
    counts.why[kind] = (counts.why[kind] ?? 0) + 1;
  };
  const releaseOpts = { ...opts, onFail: note };

  const removeEntry = async (path) => {
    const parent = path.slice(0, path.lastIndexOf('/'));
    const release = await lock(parent);
    try {
      return (await fs.delete(path)) !== false;
    } finally {
      release();
    }
  };
  /** Release the blob, then remove the entry. False (and nothing removed) when the portal did not release. */
  const removeFile = async (path, size) => {
    const hash = await advanced.pathToCID(path);
    if (!(hash instanceof Uint8Array) || !Number.isFinite(Number(size))) {
      note(hash instanceof Uint8Array ? 'no-size' : 'no-hash');
      return false;
    }
    if (!(await releaseBlob(api, hash, Number(size), releaseOpts))) {
      return false;
    }
    counts.released += 1;
    if (!(await removeEntry(path))) {
      note('entry-not-removed');
      return false;
    }
    counts.files += 1;
    return true;
  };

  for (const item of await children(fs, prefix)) {
    const name = item.name;
    const isDir = item.type === 'directory' && JOB_DIR.test(name);
    const isFile = item.type === 'file' && JOB_FILE.test(name);
    if (!isDir && !isFile) {
      continue;
    }
    counts.scanned += 1;
    const jobName = isFile ? name.slice(0, -'_attestation.json'.length) : name;
    if (exclude.has(name) || exclude.has(jobName)) {
      counts.skipped += 1;
      continue;
    }
    const path = `${prefix}/${name}`;
    try {
      if (isFile) {
        const t = recordedSecs(item.timestamp);
        if (!Number.isFinite(t) || t >= cutoff) {
          if (!(t >= cutoff)) counts.skipped += 1;
          continue;
        }
        if (counts.deleted >= maxEntries) {
          counts.more = true;
          break;
        }
        if (await removeFile(path, item.size)) {
          counts.deleted += 1;
        } else {
          counts.failed += 1;
        }
        continue;
      }
      const inside = await children(fs, path);
      const times = inside.map((c) => recordedSecs(c.timestamp));
      const plain = inside.every((c) => c.type === 'file');
      const known = times.every((t) => Number.isFinite(t));
      const newest = inside.length > 0 ? Math.max(...times) : recordedSecs(item.timestamp);
      if (!plain || !known || !Number.isFinite(newest)) {
        counts.skipped += 1; // age unknown, or not a shape this job wrote: a person decides
        continue;
      }
      if (newest >= cutoff) {
        continue;
      }
      if (counts.deleted >= maxEntries) {
        counts.more = true;
        break;
      }
      let ok = true;
      for (const child of inside) {
        if (!(await removeFile(`${path}/${child.name}`, child.size))) {
          ok = false;
        }
      }
      if (ok && (await removeEntry(path))) {
        counts.deleted += 1;
      } else {
        counts.failed += 1;
      }
    } catch (e) {
      note(`threw-${String(e?.name ?? 'Error').slice(0, 40)}`);
      counts.failed += 1;
    }
  }
  return counts;
}
