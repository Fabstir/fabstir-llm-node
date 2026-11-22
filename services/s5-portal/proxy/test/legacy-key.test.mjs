// Tests for the blob route of the S5 auth proxy: legacy (older style) id keys and the early branch that
// answers them from our own store. Run with: npm test   (from the proxy folder)
//
// The shipped file is loaded in a wrapper with stubbed http, fs, ethers and timers: no ports, no secrets, no chain.
// PROXY_FILE (relative to the proxy folder) picks the file under test; EXPECT_CDN is the download address that file
// uses. Defaults test proxy/server.js, which reads its addresses from the environment (set below).
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import { Readable } from 'node:stream';
import { EventEmitter } from 'node:events';
import { createRequire } from 'node:module';

const req0 = createRequire(import.meta.url);
const here = path.dirname(new URL(import.meta.url).pathname);
const proxyDir = path.join(here, '..');
const SRC = fs.readFileSync(path.resolve(proxyDir, process.env.PROXY_FILE || 'server.js'), 'utf8');
const CDN = process.env.EXPECT_CDN || 'https://cdn.test.invalid/s5-data/';

// Settings the kit proxy requires. The Global snapshot ignores them (its addresses are constants).
process.env.BASE_SEPOLIA_RPC_URL = 'https://rpc.test.invalid';
process.env.S5_PROXY_STORAGE_BASE = 'http://store.test.invalid/';
process.env.S5_PROXY_STORAGE_HOST = 'store.test.invalid';
process.env.S5_PROXY_CDN_BASE = CDN;

function load(headStatus) {
  const calls = { storage: [], node: [] };
  let handler;
  const fakeHttp = {
    ...http,
    createServer: (h) => { handler = h; return { listen() {} }; },
    request(a, b, c) {
      const ee = new EventEmitter();
      ee.setTimeout = () => {}; ee.destroy = () => {};
      if (typeof a === 'string') { // storageHead
        calls.storage.push(a);
        ee.end = () => setImmediate(() => { const h = typeof headStatus === 'function' ? headStatus(a) : headStatus; if (h === 0) ee.emit('error', new Error('x')); else c({ statusCode: h, resume() {} }); });
      } else { // node path
        calls.node.push(a.path);
        ee.end = () => setImmediate(() => { const r = Readable.from(['node-answer']); r.statusCode = 200; r.headers = {}; b(r); });
      }
      return ee;
    },
  };
  const fakeReq = (n) => n === 'http' ? fakeHttp : n === 'fs' ? { readFileSync: () => 'tok' } : n === 'ethers' ? { ethers: { JsonRpcProvider: class {}, Contract: class {}, verifyMessage() {} } } : req0(n);
  fakeReq.main = {};
  const mod = { exports: {} };
  new Function('require', 'module', 'exports', 'setInterval', SRC)(fakeReq, mod, mod.exports, () => {});
  return { ...mod.exports, handler, calls };
}
const { newFormatBlobKey, legacyBlobKey } = load(200);

// Real ids: READ from the reference file at run time, never copied into the repository. They embed decryption keys,
// so no assertion message may print one (rows are reported by index only).
const TSV = path.resolve(here, '../../../../docs/fabstir-v2-reference/V2-MEDIA-IDS-2026-11-16.tsv');
const haveTsv = fs.existsSync(TSV);
test('every real film id maps to a syntactically valid key', { skip: haveTsv ? false : 'reference id file not present' }, () => {
  const ids = fs.readFileSync(TSV, 'utf8').split('\n').slice(1).filter(Boolean).map((l) => l.split('\t')[0]);
  assert.ok(ids.length > 0);
  const bad = [];
  ids.forEach((id, i) => { const k = legacyBlobKey(id.split(/[?#.]/)[0]); if (!(k && /^1\/[A-Za-z0-9_-]{44}$/.test(k))) bad.push(i); });
  assert.deepEqual(bad, [], 'rows without a valid key (by index)');
});

const b58 = (buf) => { const A = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'; let n = BigInt('0x' + buf.toString('hex')); let s = ''; while (n > 0n) { s = A[Number(n % 58n)] + s; n /= 58n; } for (const x of buf) { if (x === 0) s = '1' + s; else break; } return s; };
const b32 = (buf) => { const A = 'abcdefghijklmnopqrstuvwxyz234567'; let bits = 0, v = 0, s = ''; for (const x of buf) { v = (v << 8) | x; bits += 8; while (bits >= 5) { bits -= 5; s += A[(v >> bits) & 31]; v &= (1 << bits) - 1; } } if (bits) s += A[(v << (5 - bits)) & 31]; return s; };
const mb = (bytes, pre) => pre === 'u' ? 'u' + bytes.toString('base64url') : pre === 'z' ? 'z' + b58(bytes) : 'b' + b32(bytes);
// Synthetic hash and ids: nothing here is a real id.
const H = Buffer.from(Array.from({ length: 32 }, (_, i) => (i * 7 + 3) & 255));
const KEY = '1/' + Buffer.concat([Buffer.from([0x1f]), H]).toString('base64url');
const raw = Buffer.concat([Buffer.from([0x26, 0x1f]), H, Buffer.from([0x10, 0x27])]);
const enc = Buffer.concat([Buffer.from([0xae, 0xa6, 0x12, 0x1f]), H, Buffer.alloc(60, 9)]);
const nfBytes = Buffer.concat([Buffer.from([0x5b, 0x82, 0x1f]), H]);
const nfId = 'z' + b58(nfBytes);

test('u, b, z forms of raw and encrypted ids give the same key', () => {
  for (const b of [raw, enc]) for (const p of ['u', 'b', 'z']) assert.equal(legacyBlobKey(mb(b, p)), KEY, p);
});
test('extension and query forms (as the handler strips them) give the same key', () => {
  const id = mb(enc, 'u');
  for (const suf of ['.mp4', '?x=1', '#f', '.m4s?y=2']) assert.equal(legacyBlobKey((id + suf).split(/[?#.]/)[0]), KEY);
});
test('new-format z id still goes through newFormatBlobKey, unchanged', () => {
  const nf = 'z' + b58(Buffer.concat([nfBytes, Buffer.from([1, 2, 3])]));
  assert.equal(newFormatBlobKey(nf), KEY);
  assert.equal(newFormatBlobKey(mb(raw, 'u')), null);
  assert.equal(legacyBlobKey(nf), null); // not a legacy layout
});
test('garbage, truncated, wrong type, non-BLAKE3 give null', () => {
  const bad = [null, undefined, '', 'u', 'x', 'uAAAA', 'u!!!', 'u' + 'A'.repeat(50) + '=', 'b1111', 'z0OIl', 'zz', 'Qm' + 'a'.repeat(44),
    mb(raw.subarray(0, 34), 'u'), mb(raw.subarray(0, 20), 'u'), mb(Buffer.concat([raw, Buffer.alloc(8)]), 'u'),
    mb(Buffer.concat([Buffer.from([0x26, 0x1e]), H, Buffer.from([1])]), 'u'), // non-BLAKE3 hash type
    mb(Buffer.concat([Buffer.from([0xae, 0xa6, 0x12, 0x1e]), H, Buffer.alloc(8)]), 'u'),
    mb(Buffer.concat([Buffer.from([0xae, 0xa6, 0x12, 0x1f]), H.subarray(0, 10)]), 'u'),
    mb(Buffer.concat([Buffer.from([0x27, 0x1f]), H, Buffer.from([1])]), 'u'), // wrong first byte
    mb(Buffer.concat([Buffer.from([0x5b, 0x82, 0x1e]), H, Buffer.from([1])]), 'z')];
  for (const b of bad) assert.equal(legacyBlobKey(b), null, String(b).slice(0, 12));
});

function fakeRes() { const r = { h: {}, status: null, setHeader(k, v) { r.h[k] = v; }, writeHead(s, hh) { r.status = s; r.hdrs = hh; }, end() { r.ended = true; }, on() {}, write() {}, once() {}, emit() {}, destroyed: false, headersSent: false }; return r; }
async function run(headStatus, method, url) {
  const L = load(headStatus); const res = fakeRes();
  const done = new Promise((r) => { const e = res.end; res.end = (...a) => { e(); r(); }; });
  const rq = { method, url, headers: {}, socket: { remoteAddress: '1.1.1.1' } };
  await L.handler(rq, res);
  await Promise.race([done, new Promise((r) => setTimeout(r, 100))]);
  return { res, calls: L.calls };
}
const legacyUrl = '/s5/blob/' + mb(enc, 'u');
const rawUrl = '/s5/blob/' + mb(raw, 'u');
const nfUrl = '/s5/blob/' + nfId;

for (const m of ['GET', 'HEAD']) {
  for (const u of [legacyUrl, rawUrl]) {
    test(m + ' older id + store 200 -> 307 to the download address + key, node untouched', async () => {
      const { res, calls } = await run(200, m, u);
      assert.equal(res.status, 307);
      assert.equal(res.hdrs.Location, CDN + KEY);
      assert.equal(calls.node.length, 0);
      assert.equal(calls.storage.length, 1);
    });
  }
}
for (const st of [404, 503, 0]) {
  test('GET older id + store ' + st + ' falls through to the node path', async () => {
    const { res, calls } = await run(st, 'GET', legacyUrl);
    assert.equal(calls.storage.length, 1);
    assert.equal(calls.node.length, 1);
    assert.equal(calls.node[0], legacyUrl);
    assert.equal(res.status, 200); // the node stub answered, not the proxy's own 404/307/503
  });
}
test('new-format id + store 200 still 307', async () => {
  const { res, calls } = await run(200, 'GET', nfUrl);
  assert.equal(res.status, 307); assert.equal(res.hdrs.Location, CDN + KEY); assert.equal(calls.node.length, 0);
});
test('new-format id + store 404 still answers 404 itself', async () => {
  const { res, calls } = await run(404, 'GET', nfUrl);
  assert.equal(res.status, 404); assert.equal(calls.node.length, 0); assert.equal(calls.storage.length, 1);
});
test('new-format id + store error still answers 503 itself', async () => {
  const { res, calls } = await run(0, 'GET', nfUrl);
  assert.equal(res.status, 503); assert.equal(calls.node.length, 0); assert.equal(calls.storage.length, 1);
});
test('unparseable id never queries the store, goes to node', async () => {
  const { calls } = await run(200, 'GET', '/s5/blob/uNOTANID');
  assert.equal(calls.storage.length, 0); assert.equal(calls.node.length, 1);
});
