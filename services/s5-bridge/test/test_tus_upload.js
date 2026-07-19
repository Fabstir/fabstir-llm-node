// Outbound tus client tests — a mock S5 node speaking the exact http_api tus
// dialect (create → Location → offset PATCHes → server-side hash check).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { Buffer } from 'node:buffer';
import {
  installTusLargeUpload,
  tusMetadataHash,
  tusUploadToPortal,
} from '../src/tus_upload.js';

const MULTIHASH_BLAKE3 = 0x1e;

function makeMultihash(seed = 7) {
  const mh = new Uint8Array(33);
  mh[0] = MULTIHASH_BLAKE3;
  for (let i = 1; i < 33; i++) mh[i] = (seed * i) % 256;
  return mh;
}

/** Mock S5 node: records requests, accumulates chunk bytes, serves the dialect. */
function mockS5TusServer({ failCreates = 0 } = {}) {
  const state = { creates: [], patches: [], bytes: Buffer.alloc(0), failCreates };
  const server = createServer((req, res) => {
    if (req.method === 'POST' && req.url === '/s5/upload/tus') {
      state.creates.push({
        auth: req.headers['authorization'],
        length: req.headers['upload-length'],
        metadata: req.headers['upload-metadata'],
      });
      if (state.failCreates > 0) {
        state.failCreates -= 1;
        res.writeHead(500).end('portal exploded');
        return;
      }
      res.writeHead(201, { Location: '/s5/upload/tus/upl-1' }).end();
      return;
    }
    if (req.method === 'PATCH' && req.url === '/s5/upload/tus/upl-1') {
      const chunks = [];
      req.on('data', (c) => chunks.push(c));
      req.on('end', () => {
        const body = Buffer.concat(chunks);
        state.patches.push({
          offset: Number(req.headers['upload-offset']),
          size: body.length,
          contentType: req.headers['content-type'],
        });
        state.bytes = Buffer.concat([state.bytes, body]);
        res.writeHead(204, {
          'Tus-Resumable': '1.0.0',
          'Upload-Offset': String(state.bytes.length),
        }).end();
      });
      return;
    }
    res.writeHead(404).end();
  });
  return { server, state };
}

function portalFor(port) {
  return {
    host: `127.0.0.1:${port}`,
    headers: { Authorization: 'Bearer test-token' },
    apiURL(path) {
      return `http://127.0.0.1:${port}/s5/${path}`;
    },
  };
}

const listen = (server) =>
  new Promise((r) => server.listen(0, '127.0.0.1', () => r(server.address().port)));

test('metadata value survives the S5 double decode (b64std → utf8 → b64url)', () => {
  const mh = makeMultihash();
  const value = tusMetadataHash(mh);
  const inner = Buffer.from(value, 'base64').toString('utf8'); // node: base64.decode → utf8
  const decoded = Buffer.from(inner, 'base64url'); // node: Multihash.fromBase64Url
  assert.deepEqual(new Uint8Array(decoded), mh);
});

test('uploads in offset-ordered chunks with the dialect headers', async () => {
  const { server, state } = mockS5TusServer();
  const port = await listen(server);
  try {
    const data = Buffer.from('abcdefghijklmnopqrst'); // 20 bytes
    const mh = makeMultihash();
    await tusUploadToPortal(portalFor(port), new Blob([data]), mh, { chunkSize: 8 });

    assert.equal(state.creates.length, 1);
    assert.equal(state.creates[0].auth, 'Bearer test-token');
    assert.equal(state.creates[0].length, '20');
    assert.equal(state.creates[0].metadata, `hash ${tusMetadataHash(mh)}`);

    assert.deepEqual(
      state.patches.map((p) => [p.offset, p.size]),
      [
        [0, 8],
        [8, 8],
        [16, 4],
      ],
    );
    assert.ok(state.patches.every((p) => p.contentType === 'application/offset+octet-stream'));
    assert.deepEqual(state.bytes, data); // byte-identical arrival
  } finally {
    server.close();
  }
});

test('installTusLargeUpload: small blobs keep the original path untouched', async () => {
  let origCalls = 0;
  const api = {
    accountConfigs: {},
    crypto: {},
    uploadBlob: async () => {
      origCalls += 1;
      return 'original-result';
    },
  };
  installTusLargeUpload(api, { threshold: 100 });
  const out = await api.uploadBlob(new Blob([Buffer.alloc(99)]));
  assert.equal(out, 'original-result');
  assert.equal(origCalls, 1);
});

test('installTusLargeUpload: at-threshold blobs go tus and return the blob identifier', async () => {
  const { server, state } = mockS5TusServer();
  const port = await listen(server);
  try {
    const data = Buffer.alloc(120, 0x5a);
    const mh = makeMultihash(3);
    const api = {
      accountConfigs: { p1: portalFor(port) },
      crypto: { hashBlake3Blob: async () => mh.slice(1) }, // library returns the raw 32-byte hash
      uploadBlob: async () => {
        throw new Error('single-shot path must not run for large blobs');
      },
    };
    installTusLargeUpload(api, { threshold: 100, chunkSize: 64, logger: { log() {}, error() {} } });

    const bid = await api.uploadBlob(new Blob([data]));
    assert.deepEqual(new Uint8Array(bid.hash), mh); // returned identifier: modern 0x1e
    assert.equal(bid.size, 120);
    assert.equal(state.creates[0].length, '120');
    assert.deepEqual(state.bytes, data);

    // The WIRE metadata must carry the DEPRECATED 0x1f type byte + the same
    // digest — the stock S5 node's tus completion compares against
    // lib5's mhashBlake3Default (0x1f); modern 0x1e is rejected there, while
    // reads normalise 0x1f→0x1e in s5.js BlobIdentifier.decode. Session-924-era
    // probe proof: 0x1e on the wire → "Invalid hash found" at completion.
    const legacy = Uint8Array.from(mh);
    legacy[0] = 0x1f;
    assert.equal(state.creates[0].metadata, `hash ${tusMetadataHash(legacy)}`);
  } finally {
    server.close();
  }
});

test('installTusLargeUpload: retries the portal ladder and succeeds after failures', async () => {
  const { server, state } = mockS5TusServer({ failCreates: 2 });
  const port = await listen(server);
  try {
    const mh = makeMultihash(5);
    const api = {
      accountConfigs: { p1: portalFor(port) },
      crypto: { hashBlake3Blob: async () => mh.slice(1) },
      uploadBlob: async () => {
        throw new Error('unreachable');
      },
    };
    installTusLargeUpload(api, { threshold: 10, chunkSize: 1024, logger: { log() {}, error() {} } });

    const data = Buffer.alloc(50, 1);
    const bid = await api.uploadBlob(new Blob([data]));
    assert.equal(bid.size, 50);
    assert.equal(state.creates.length, 3); // two failures, third try lands
    assert.deepEqual(state.bytes, data);
  } finally {
    server.close();
  }
});

test('installTusLargeUpload: exhausting every retry surfaces one clear error', async () => {
  const { server } = mockS5TusServer({ failCreates: 99 });
  const port = await listen(server);
  try {
    const mh = makeMultihash(9);
    const api = {
      accountConfigs: { p1: portalFor(port) },
      crypto: { hashBlake3Blob: async () => mh.slice(1) },
      uploadBlob: async () => {
        throw new Error('unreachable');
      },
    };
    installTusLargeUpload(api, { threshold: 10, logger: { log() {}, error() {} } });
    await assert.rejects(
      api.uploadBlob(new Blob([Buffer.alloc(20)])),
      /Failed to tus-upload blob with 3 tries/,
    );
  } finally {
    server.close();
  }
});
