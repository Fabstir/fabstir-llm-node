// Retention prune tests: a fake S5 filesystem with the shapes s5.js 0.9.0-beta.55 hands back (`fs.list` yields
// name, type, size and a time in MILLISECONDS, or an ISO string from a sharded directory, and no hash; the hash
// comes from the advanced client's `pathToCID`), and a mock portal speaking the accounts API's delete call.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { Buffer } from 'node:buffer';
import { BlobIdentifier } from '@julesl23/s5js/dist/src/identifier/blob.js';
import {
  pruneOlderThan, portalCid, releaseBlob, recordedSecs, MAX_ENTRIES_PER_RUN,
} from '../src/prune.js';

const DAY = 86_400;
const NOW = 1_800_000_000;
const OLD = NOW - 31 * DAY;
const YOUNG = NOW - 29 * DAY;

function hashOf(seed) {
  return Uint8Array.from({ length: 32 }, (_, i) => (seed * (i + 3)) % 256);
}
function file(seed, timestamp, size = 1000 + seed) {
  return { type: 'file', data: { hash: hashOf(seed), size, timestamp } };
}
function dir(entries) {
  return { type: 'directory', data: { ts_seconds: undefined }, entries };
}

/** Fake fs: a tree of { name: file | dir }, with s5.js's delete rule (a directory goes only when empty). */
function fakeFs(tree) {
  const at = (path) => path.split('/').reduce((n, seg) => n?.entries?.[seg], { entries: tree });
  const deletes = [];
  return {
    tree,
    deletes,
    async *list(path) {
      const node = at(path);
      for (const [name, v] of Object.entries(node?.entries ?? {})) {
        const t = v.type === 'file' ? v.data.timestamp : v.data.ts_seconds;
        // as s5.js: seconds on the reference, milliseconds in the listing; `iso` stands for a sharded directory
        const timestamp = t === undefined ? undefined : v.iso ? new Date(t * 1000).toISOString() : t * 1000;
        yield v.type === 'file' ? { name, type: 'file', size: v.data.size, timestamp } : { name, type: 'directory', timestamp };
      }
    },
    hashAt(path) {
      return at(path)?.data?.hash;
    },
    async delete(path) {
      const cut = path.lastIndexOf('/');
      const parent = at(path.slice(0, cut));
      const name = path.slice(cut + 1);
      const node = parent?.entries?.[name];
      if (!node || (node.type === 'directory' && Object.keys(node.entries).length > 0)) {
        return false;
      }
      delete parent.entries[name];
      deletes.push(path);
      return true;
    },
  };
}

/** Mock portal: records every DELETE /s5/delete/<cid> with its token; `refuse` cids answer 500. */
async function mockPortal({ refuse = new Set() } = {}) {
  const calls = [];
  const server = createServer((req, res) => {
    const m = /^\/s5\/delete\/(.+)$/.exec(req.url);
    if (req.method !== 'DELETE' || !m) {
      res.writeHead(405).end();
      return;
    }
    calls.push({ cid: m[1], auth: req.headers['authorization'] });
    res.writeHead(refuse.has(m[1]) ? 500 : 200).end();
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const host = `127.0.0.1:${server.address().port}`;
  const portal = {
    host,
    headers: { Authorization: 'Bearer host-token' },
    apiURL: (p) => `http://${host}/s5/${p}`,
  };
  return { calls, portal, close: () => new Promise((r) => server.close(r)) };
}

function world(portal) {
  const tree = {
    home: dir({
      ltx: dir({
        job_old: dir({ 'frame_00000.bin': file(1, OLD), 'manifest.json': file(2, OLD - 5) }),
        'job_old_attestation.json': { ...file(3, OLD), iso: true },
        // one file written inside the period keeps the whole job
        job_mixed: dir({ 'frame_00000.bin': file(4, OLD), 'frame_00001.bin': file(5, YOUNG) }),
        job_young: dir({ 'frame_00000.bin': file(6, YOUNG) }),
        'job_young_attestation.json': file(7, YOUNG),
        job_live: dir({ 'frame_00000.bin': file(8, OLD) }),
        job_undated: dir({ 'frame_00000.bin': file(9, undefined) }),
        job_nested: dir({ sub: dir({ 'x.bin': file(10, OLD) }) }),
        'notes.txt': file(11, OLD),
        other: dir({ 'a.bin': file(12, OLD) }),
      }),
      checkpoints: dir({ job_old: dir({ 'delta.json': file(13, OLD) }) }),
    }),
  };
  const fs = fakeFs(tree);
  const advanced = {
    async pathToCID(path) {
      const hash = fs.hashAt(path);
      if (!hash) throw new Error(`Path not found: ${path}`);
      return hash;
    },
  };
  return { fs, advanced, s5: { fs, apiWithIdentity: { accountConfigs: { a: portal } } } };
}

const ARGS = { prefix: 'home/ltx', olderThanSecs: 30 * DAY, exclude: ['job_live'] };

test('old job results are deleted and every blob is released on the portal first', async () => {
  const p = await mockPortal();
  try {
    const { fs, s5, advanced } = world(p.portal);
    const locks = [];
    const lock = async (d) => {
      locks.push(d);
      return () => {};
    };
    const counts = await pruneOlderThan(s5, ARGS, { nowSecs: NOW, lock, advanced });
    assert.deepEqual(counts, {
      scanned: 8, deleted: 2, files: 3, released: 3, failed: 0, skipped: 3, more: false,
    });
    const ltx = fs.tree.home.entries.ltx.entries;
    assert.deepEqual(Object.keys(ltx).sort(), [
      'job_live', 'job_mixed', 'job_nested', 'job_undated', 'job_young',
      'job_young_attestation.json', 'notes.txt', 'other',
    ]);
    assert.equal(Object.keys(ltx.job_mixed.entries).length, 2, 'a job with one young file is whole');
    assert.ok(fs.tree.home.entries.checkpoints.entries.job_old, 'nothing outside the tree is touched');
    // the three blobs of the old job, each released under the host's token, in the stock node's cid form
    assert.deepEqual(
      p.calls.map((c) => c.cid).sort(),
      [portalCid(hashOf(1), 1001), portalCid(hashOf(2), 1002), portalCid(hashOf(3), 1003)].sort(),
    );
    assert.ok(p.calls.every((c) => c.auth === 'Bearer host-token'));
    // the job folder went last, after its files; each removal held its parent directory's lock
    assert.ok(fs.deletes.indexOf('home/ltx/job_old') > fs.deletes.indexOf('home/ltx/job_old/manifest.json'));
    assert.ok(locks.includes('home/ltx/job_old') && locks.includes('home/ltx'));
    // a second run finds nothing left to do
    const again = await pruneOlderThan(s5, ARGS, { nowSecs: NOW, advanced });
    assert.equal(again.deleted + again.files + again.failed, 0);
  } finally {
    await p.close();
  }
});

test('an entry whose blob the portal will not release is kept, hash and all, for the next run', async () => {
  const p = await mockPortal({ refuse: new Set([portalCid(hashOf(2), 1002)]) });
  try {
    const { fs, s5, advanced } = world(p.portal);
    const counts = await pruneOlderThan(s5, ARGS, { nowSecs: NOW, advanced });
    assert.equal(counts.failed, 1);
    assert.equal(counts.deleted, 1, 'the attestation file');
    const job = fs.tree.home.entries.ltx.entries.job_old;
    assert.deepEqual(Object.keys(job.entries), ['manifest.json'], 'the unreleased file and its folder stay');
    assert.deepEqual(job.entries['manifest.json'].data.hash, hashOf(2));
  } finally {
    await p.close();
  }
});

test('with no portal account nothing is deleted at all', async () => {
  const { fs, s5, advanced } = world(null);
  s5.apiWithIdentity.accountConfigs = {};
  const counts = await pruneOlderThan(s5, ARGS, { nowSecs: NOW, advanced });
  assert.equal(counts.deleted, 0);
  assert.equal(counts.failed, 2);
  assert.deepEqual(fs.deletes, []);
  assert.equal(await releaseBlob(s5.apiWithIdentity, hashOf(1), 5), false);
});

test('the run is bounded, the tree is fixed and the period has a floor', async () => {
  const p = await mockPortal();
  try {
    const { s5, advanced } = world(p.portal);
    const one = await pruneOlderThan(s5, { ...ARGS, maxEntries: 1 }, { nowSecs: NOW, advanced });
    assert.equal(one.deleted, 1);
    assert.equal(one.more, true);
    assert.equal(MAX_ENTRIES_PER_RUN, 500);
    for (const bad of [
      { ...ARGS, prefix: 'home' },
      { ...ARGS, prefix: 'home/checkpoints' },
      { ...ARGS, prefix: 'home/ltx/../checkpoints' },
      { ...ARGS, olderThanSecs: 0 },
      { ...ARGS, olderThanSecs: DAY - 1 },
      { ...ARGS, olderThanSecs: '2592000' },
      {},
    ]) {
      await assert.rejects(() => pruneOlderThan(s5, bad, { nowSecs: NOW, advanced }), RangeError);
    }
  } finally {
    await p.close();
  }
});

test('the cid is the stock node form of the same blob s5.js names', () => {
  const cid = portalCid(hashOf(1), 70_000);
  const bytes = Buffer.from(cid.slice(1), 'base64url');
  assert.equal(cid[0], 'u');
  assert.deepEqual([...bytes.subarray(0, 2)], [0x26, 0x1f]);
  assert.deepEqual([...bytes.subarray(2, 34)], [...hashOf(1)]);
  assert.deepEqual([...bytes.subarray(34)], [0x70, 0x11, 0x01], '70000, little-endian, no trailing zeros');
  // s5.js reads it back as the very blob (its legacy branch turns 0x1f into 0x1e)
  const back = BlobIdentifier.decode(cid);
  assert.equal(back.size, 70_000);
  assert.deepEqual([...back.hash], [0x1e, ...hashOf(1)]);
  // a 33-byte multihash names the same blob
  assert.equal(portalCid(Uint8Array.from([0x1e, ...hashOf(1)]), 70_000), cid);
  assert.throws(() => portalCid(new Uint8Array(5), 1));
});

test('recorded times are read in every form s5.js gives them, and anything else is unknown', async () => {
  assert.equal(recordedSecs(1_700_000_000_000), 1_700_000_000, 'milliseconds (a plain directory listing)');
  assert.equal(recordedSecs('2023-11-14T22:13:20.000Z'), 1_700_000_000, 'ISO (a sharded directory listing)');
  assert.equal(recordedSecs(1_700_000_000), 1_700_000_000, 'seconds');
  for (const unknown of [undefined, null, 0, -5, NaN, '', 'soon', {}]) {
    assert.ok(Number.isNaN(recordedSecs(unknown)), String(unknown));
  }
  // without the advanced client there is no hash to release: refuse to start rather than delete entries
  const p = await mockPortal();
  try {
    const { fs, s5 } = world(p.portal);
    await assert.rejects(() => pruneOlderThan(s5, ARGS, { nowSecs: NOW }), /advanced client/);
    assert.deepEqual(fs.deletes, []);
  } finally {
    await p.close();
  }
});
