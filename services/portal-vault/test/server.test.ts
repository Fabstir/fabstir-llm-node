// The HTTP layer: routing, JSON errors, body limit, rate limit and
// X-Forwarded-For (R7), CORS (R8), and the end-to-end flow of task 10. The
// server is started on an ephemeral loopback port with the in-memory store and
// a fake verifier; nothing leaves the machine.
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { afterEach, describe, expect, it } from 'vitest';
import { RateLimiter } from '../src/ratelimit.js';
import { clientAddress, createVaultServer } from '../src/server.js';
import { ADDR_A, ADDR_A_LC, CHAIN, fakeSign, makeEnv, sha256Hex, type TestEnv } from './helpers.js';

interface Running {
  env: TestEnv;
  base: string;
  port: number;
  close: () => Promise<void>;
}

const running: Running[] = [];

async function start(opts: { allowedOrigins?: string[]; perMinute?: number; maxBodyBytes?: number; verifyPerMinute?: number } = {}): Promise<Running> {
  const env = makeEnv(opts.verifyPerMinute !== undefined ? { verifyPerMinute: opts.verifyPerMinute } : {});
  const server = createVaultServer({
    deps: env.deps,
    rateLimiter: new RateLimiter({ perMinute: opts.perMinute ?? 1000, now: () => env.clock.t }),
    allowedOrigins: opts.allowedOrigins ?? [],
    ...(opts.maxBodyBytes !== undefined ? { maxBodyBytes: opts.maxBodyBytes } : {}),
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const port = (server.address() as AddressInfo).port;
  const r: Running = {
    env,
    port,
    base: `http://127.0.0.1:${port}`,
    close: () =>
      new Promise<void>((resolve) => {
        server.close(() => resolve());
        server.closeAllConnections();
      }),
  };
  running.push(r);
  return r;
}

afterEach(async () => {
  while (running.length) await running.pop()!.close();
});

interface Reply {
  status: number;
  headers: http.IncomingHttpHeaders;
  text: string;
  json: any;
}

/** A plain node:http client, so tests control every header (fetch forbids
 *  some) and can send bodies of any size. */
function call(r: Running, method: string, path: string, opts: { body?: unknown; raw?: string | Buffer; headers?: Record<string, string> } = {}): Promise<Reply> {
  return new Promise((resolve, reject) => {
    const payload = opts.raw !== undefined ? opts.raw : opts.body !== undefined ? JSON.stringify(opts.body) : undefined;
    const headers: Record<string, string> = { ...(opts.headers ?? {}) };
    if (payload !== undefined) {
      headers['Content-Type'] ??= 'application/json';
      headers['Content-Length'] = String(Buffer.byteLength(payload));
    }
    const req = http.request({ host: '127.0.0.1', port: r.port, method, path, headers, agent: false }, (res) => {
      const chunks: Buffer[] = [];
      res.on('data', (c: Buffer) => chunks.push(c));
      res.on('end', () => {
        const text = Buffer.concat(chunks).toString('utf8');
        let json: unknown;
        try {
          json = JSON.parse(text);
        } catch {
          json = undefined;
        }
        resolve({ status: res.statusCode ?? 0, headers: res.headers, text, json });
      });
    });
    req.on('error', reject);
    if (payload !== undefined) req.write(payload);
    req.end();
  });
}

const post = (r: Running, path: string, body: unknown, headers?: Record<string, string>) =>
  call(r, 'POST', `/vault/v1/${path}`, { body, ...(headers ? { headers } : {}) });

describe('end to end (in-memory store, fake verifier)', () => {
  it('challenge, open (404), replace from 0, open, current with token, replace from 1, old token refused, conflict', async () => {
    const r = await start();
    const vault1 = 'e2e-wrapped-vault-1:' + 'p'.repeat(300);
    const vault2 = 'e2e-wrapped-vault-2:' + 'q'.repeat(300);
    const currentPath = `/vault/v1/current?address=${ADDR_A}&chainId=${CHAIN}`;

    // challenge
    const c1 = await post(r, 'challenge', { address: ADDR_A, chainId: CHAIN, purpose: 'read' });
    expect(c1.status).toBe(200);
    expect(c1.headers['content-type']).toContain('application/json');
    expect(c1.json.address).toBe(ADDR_A_LC);

    // open: nothing stored yet
    const o1 = await post(r, 'open', { address: ADDR_A, chainId: CHAIN, nonce: c1.json.nonce, signature: fakeSign(ADDR_A, { message: c1.json.message }), form: 'message' });
    expect(o1.status).toBe(404);
    expect(o1.json).toEqual({ error: 'no-vault' });

    // replace from 0
    const c2 = await post(r, 'challenge', { address: ADDR_A, chainId: CHAIN, purpose: 'replace', vaultSha256: sha256Hex(vault1), replaces: 0 });
    const r1 = await post(r, 'replace', { address: ADDR_A, chainId: CHAIN, nonce: c2.json.nonce, signature: fakeSign(ADDR_A, { message: c2.json.message }), form: 'message', vault: vault1, replaces: 0 });
    expect(r1.status).toBe(200);
    expect(r1.json.version).toBe(1);

    // open: now present (typed-data form this time)
    const c3 = await post(r, 'challenge', { address: ADDR_A, chainId: CHAIN, purpose: 'read' });
    expect(c3.json.nonce).not.toBe(c1.json.nonce);
    const o2 = await post(r, 'open', { address: ADDR_A, chainId: CHAIN, nonce: c3.json.nonce, signature: fakeSign(ADDR_A, { typedData: c3.json.typedData }), form: 'typedData' });
    expect(o2.status).toBe(200);
    expect(o2.json.vault).toBe(vault1);
    expect(o2.json.version).toBe(1);
    const oldToken = o2.json.readToken as string;

    // current with token
    const g1 = await call(r, 'GET', currentPath, { headers: { Authorization: `Bearer ${oldToken}` } });
    expect(g1.status).toBe(200);
    expect(g1.json).toEqual({ version: 1, updatedAt: o2.json.updatedAt, vault: vault1 });
    expect(g1.headers['cache-control']).toBe('no-store');

    // replace from 1
    r.env.clock.t += 2000;
    const c4 = await post(r, 'challenge', { address: ADDR_A, chainId: CHAIN, purpose: 'replace', vaultSha256: sha256Hex(vault2), replaces: 1 });
    const r2 = await post(r, 'replace', { address: ADDR_A, chainId: CHAIN, nonce: c4.json.nonce, signature: fakeSign(ADDR_A, { message: c4.json.message }), form: 'message', vault: vault2, replaces: 1 });
    expect(r2.status).toBe(200);
    expect(r2.json.version).toBe(2);

    // old token refused, new one accepted
    const g2 = await call(r, 'GET', currentPath, { headers: { Authorization: `Bearer ${oldToken}` } });
    expect(g2.status).toBe(401);
    expect(g2.json).toEqual({ error: 'token-invalid' });
    const g3 = await call(r, 'GET', currentPath, { headers: { Authorization: `Bearer ${r2.json.readToken}` } });
    expect(g3.status).toBe(200);
    expect(g3.json.vault).toBe(vault2);

    // conflict: replacing version 1 again
    const c5 = await post(r, 'challenge', { address: ADDR_A, chainId: CHAIN, purpose: 'replace', vaultSha256: sha256Hex(vault1), replaces: 1 });
    const r3 = await post(r, 'replace', { address: ADDR_A, chainId: CHAIN, nonce: c5.json.nonce, signature: fakeSign(ADDR_A, { message: c5.json.message }), form: 'message', vault: vault1, replaces: 1 });
    expect(r3.status).toBe(409);
    expect(r3.json).toEqual({ error: 'version-conflict', currentVersion: 2 });
    expect((await r.env.store.readCurrent(CHAIN, ADDR_A_LC))!.vault).toBe(vault2);
    expect((await r.env.store.readPrevious(CHAIN, ADDR_A_LC))!.vault).toBe(vault1);
  });

  it('passes a 503 from the signature check through with Retry-After', async () => {
    const r = await start();
    const c = await post(r, 'challenge', { address: ADDR_A, chainId: CHAIN, purpose: 'read' });
    r.env.verifier.mode = 'unavailable';
    const o = await post(r, 'open', { address: ADDR_A, chainId: CHAIN, nonce: c.json.nonce, signature: fakeSign(ADDR_A, { message: c.json.message }), form: 'message' });
    expect(o.status).toBe(503);
    expect(o.json).toEqual({ error: 'signature-check-unavailable' });
    expect(Number(o.headers['retry-after'])).toBeGreaterThan(0);
  });

  it('answers health with no account data', async () => {
    const r = await start();
    const h = await call(r, 'GET', '/vault/v1/health');
    expect(h.status).toBe(200);
    expect(h.json).toEqual({ status: 'ok', region: 'uk', portal: 'uk.portal.example', chainId: CHAIN, storage: 'ok' });
  });
});

describe('routing and JSON errors', () => {
  it('answers unknown paths with 404 not-found, as JSON', async () => {
    const r = await start();
    for (const path of ['/', '/vault', '/vault/v1', '/vault/v1/', '/vault/v1/delete', '/vault/v2/health', '/vault/v1/health/x', '/vault/v1//health']) {
      const res = await call(r, 'GET', path);
      expect(res.status).toBe(404);
      expect(res.json).toEqual({ error: 'not-found' });
    }
  });

  it('answers a wrong method with 405 and Allow', async () => {
    const r = await start();
    const a = await call(r, 'GET', '/vault/v1/open');
    expect(a.status).toBe(405);
    expect(a.json).toEqual({ error: 'method-not-allowed' });
    expect(a.headers.allow).toBe('POST, OPTIONS');
    const b = await call(r, 'POST', '/vault/v1/current', { body: {} });
    expect(b.status).toBe(405);
    expect(b.headers.allow).toBe('GET, OPTIONS');
    expect((await call(r, 'DELETE', '/vault/v1/current')).status).toBe(405);
  });

  it('answers a body that is not JSON with 400 invalid-request', async () => {
    const r = await start();
    for (const raw of ['', '{', 'not json', '"text"', '[1,2]', 'null']) {
      const res = await call(r, 'POST', '/vault/v1/challenge', { raw });
      expect(res.status).toBe(400);
      expect(res.json).toEqual({ error: 'invalid-request' });
    }
  });

  it('reads address and chain id from the query of /current', async () => {
    const r = await start();
    const missing = await call(r, 'GET', '/vault/v1/current', { headers: { Authorization: 'Bearer a.b' } });
    expect(missing.status).toBe(400);
    const badToken = await call(r, 'GET', `/vault/v1/current?address=${ADDR_A}&chainId=${CHAIN}`, { headers: { Authorization: 'Bearer a.b' } });
    expect(badToken.status).toBe(401);
    const noHeader = await call(r, 'GET', `/vault/v1/current?address=${ADDR_A}&chainId=${CHAIN}`);
    expect(noHeader.status).toBe(401);
    const otherChain = await call(r, 'GET', `/vault/v1/current?address=${ADDR_A}&chainId=8453`, { headers: { Authorization: 'Bearer a.b' } });
    expect(otherChain.status).toBe(400);
    expect(otherChain.json).toEqual({ error: 'unsupported-chain' });
  });

  it('marks every answer no-store and nosniff', async () => {
    const r = await start();
    for (const res of [await call(r, 'GET', '/vault/v1/health'), await call(r, 'GET', '/nowhere'), await post(r, 'challenge', {})]) {
      expect(res.headers['cache-control']).toBe('no-store');
      expect(res.headers['x-content-type-options']).toBe('nosniff');
    }
  });
});

describe('body limit (R7)', () => {
  it('refuses a body over 64 KiB with 413, by Content-Length and by actual size, and accepts one just under', async () => {
    const r = await start();
    const over = JSON.stringify({ pad: 'x'.repeat(64 * 1024) });
    const declared = await call(r, 'POST', '/vault/v1/challenge', { raw: over });
    expect(declared.status).toBe(413);
    expect(declared.json).toEqual({ error: 'body-too-large' });

    // No Content-Length (chunked): the limit is enforced on what arrives.
    const chunked = await new Promise<Reply>((resolve, reject) => {
      const req = http.request({ host: '127.0.0.1', port: r.port, method: 'POST', path: '/vault/v1/challenge', agent: false, headers: { 'Content-Type': 'application/json' } }, (res) => {
        const chunks: Buffer[] = [];
        res.on('data', (c: Buffer) => chunks.push(c));
        res.on('end', () => {
          const text = Buffer.concat(chunks).toString('utf8');
          resolve({ status: res.statusCode ?? 0, headers: res.headers, text, json: JSON.parse(text) });
        });
      });
      req.on('error', reject);
      req.write(over.slice(0, 40_000));
      req.write(over.slice(40_000));
      req.end();
    });
    expect(chunked.status).toBe(413);
    expect(chunked.json).toEqual({ error: 'body-too-large' });

    const under = JSON.stringify({ pad: 'x'.repeat(64 * 1024 - 100) });
    expect(Buffer.byteLength(under)).toBeLessThanOrEqual(64 * 1024);
    const ok = await call(r, 'POST', '/vault/v1/challenge', { raw: under });
    expect(ok.status).toBe(400); // read in full, then refused for its content
    expect(ok.json).toEqual({ error: 'invalid-request' });
  });

  it('a vault at the default limit fits inside the body limit', async () => {
    const r = await start();
    const vault = 'v'.repeat(32768);
    const c = await post(r, 'challenge', { address: ADDR_A, chainId: CHAIN, purpose: 'replace', vaultSha256: sha256Hex(vault), replaces: 0 });
    const res = await post(r, 'replace', { address: ADDR_A, chainId: CHAIN, nonce: c.json.nonce, signature: fakeSign(ADDR_A, { message: c.json.message }), form: 'message', vault, replaces: 0 });
    expect(res.status).toBe(200);
    const tooBig = await post(r, 'replace', { address: ADDR_A, chainId: CHAIN, nonce: c.json.nonce, signature: '0x' + 'ee'.repeat(65), form: 'message', vault: vault + 'v', replaces: 1 });
    expect(tooBig.status).toBe(413);
    expect(tooBig.json).toEqual({ error: 'vault-too-large' });
  });
});

describe('rate limit (R7)', () => {
  it('answers 429 rate-limited with Retry-After past the limit, and opens again after a minute', async () => {
    const r = await start({ perMinute: 3 });
    const probe = () => post(r, 'challenge', {});
    for (let i = 0; i < 3; i++) expect((await probe()).status).toBe(400);
    const refused = await probe();
    expect(refused.status).toBe(429);
    expect(refused.json).toEqual({ error: 'rate-limited' });
    expect(Number(refused.headers['retry-after'])).toBeGreaterThan(0);
    r.env.clock.t += 60_000;
    expect((await probe()).status).toBe(400);
  });

  it('counts per forwarded client when the connection comes from loopback', async () => {
    const r = await start({ perMinute: 1 });
    const as = (ip: string) => post(r, 'challenge', {}, { 'X-Forwarded-For': ip });
    expect((await as('203.0.113.5')).status).toBe(400);
    expect((await as('203.0.113.5')).status).toBe(429);
    expect((await as('203.0.113.6')).status).toBe(400);
    expect((await as('2001:db8::1')).status).toBe(400);
    expect((await as('2001:db8::1')).status).toBe(429);
  });

  it('counts an IPv6 client by its /64, and an IPv4-mapped address as IPv4 (v0.2)', async () => {
    const r = await start({ perMinute: 1 });
    const as = (ip: string) => post(r, 'challenge', {}, { 'X-Forwarded-For': ip });
    expect((await as('2001:db8:aa:bb::1')).status).toBe(400);
    expect((await as('2001:db8:aa:bb:1234:5678:9abc:def0')).status).toBe(429); // same /64
    expect((await as('2001:db8:aa:bc::1')).status).toBe(400); // the next /64
    expect((await as('203.0.113.77')).status).toBe(400);
    expect((await as('::ffff:203.0.113.77')).status).toBe(429); // the same IPv4 client
  });

  it('leaves /health outside the limiter (v0.2)', async () => {
    const r = await start({ perMinute: 2 });
    for (let i = 0; i < 10; i++) expect((await call(r, 'GET', '/vault/v1/health')).status).toBe(200);
    // and health calls have not used up the client's allowance
    expect((await post(r, 'challenge', {})).status).toBe(400);
    expect((await post(r, 'challenge', {})).status).toBe(400);
    expect((await post(r, 'challenge', {})).status).toBe(429);
    expect((await call(r, 'GET', '/vault/v1/health')).status).toBe(200);
  });

  it('asks the storage at most once every few seconds for /health, however often it is called (v0.2)', async () => {
    const r = await start({ perMinute: 2 });
    for (let i = 0; i < 25; i++) expect((await call(r, 'GET', '/vault/v1/health')).json.storage).toBe('ok');
    expect(r.env.store.pings).toBe(1);
    r.env.store.failing = true;
    expect((await call(r, 'GET', '/vault/v1/health')).json.storage).toBe('ok'); // still the remembered answer
    r.env.clock.t += 5000;
    expect((await call(r, 'GET', '/vault/v1/health')).json.storage).toBe('error');
    expect(r.env.store.pings).toBe(2);
  });

  it('logs a 429 with the client key (v0.2)', async () => {
    const r = await start({ perMinute: 1 });
    await post(r, 'challenge', {}, { 'X-Forwarded-For': '2001:db8:aa:bb::1' });
    await post(r, 'challenge', {}, { 'X-Forwarded-For': '2001:db8:aa:bb::2' });
    expect(r.env.logs.at(-1)).toEqual({ call: 'http', outcome: 'rate-limited', status: 429, ms: 0, client: '2001:db8:aa:bb::/64' });
  });

  it('caps live challenges per client over HTTP: client A refused, client B served (v0.2)', async () => {
    const r = await start();
    const addr = (i: number) => '0x' + i.toString(16).padStart(40, '0');
    const ask = (i: number, ip: string) => post(r, 'challenge', { address: addr(i), chainId: CHAIN, purpose: 'read' }, { 'X-Forwarded-For': ip });
    for (let i = 1; i <= 20; i++) expect((await ask(i, '2001:db8:1:1::' + i.toString(16))).status).toBe(200); // one /64, many addresses
    const refused = await ask(21, '2001:db8:1:1::ffff');
    expect(refused.status).toBe(429);
    expect(refused.json).toEqual({ error: 'too-many-challenges' });
    expect(r.env.logs.at(-1)).toMatchObject({ call: 'challenge', status: 429, client: '2001:db8:1:1::/64' });
    expect((await ask(21, '203.0.113.9')).status).toBe(200);
  });

  it('caps verifications per client per minute over HTTP (v0.2)', async () => {
    const r = await start({ verifyPerMinute: 2 });
    const c = await post(r, 'challenge', { address: ADDR_A, chainId: CHAIN, purpose: 'read' });
    const attempt = (ip: string) =>
      post(r, 'open', { address: ADDR_A, chainId: CHAIN, nonce: c.json.nonce, signature: '0x' + 'ee'.repeat(65), form: 'message' }, { 'X-Forwarded-For': ip });
    expect((await attempt('203.0.113.5')).status).toBe(401);
    expect((await attempt('203.0.113.5')).status).toBe(401);
    const refused = await attempt('203.0.113.5');
    expect(refused.status).toBe(429);
    expect(refused.json).toEqual({ error: 'rate-limited' });
    expect(Number(refused.headers['retry-after'])).toBeGreaterThan(0);
    expect((await attempt('203.0.113.6')).status).toBe(401);
  });
});

describe('clientAddress (R7)', () => {
  it('trusts X-Forwarded-For only from the loopback address', () => {
    expect(clientAddress('127.0.0.1', '203.0.113.5')).toBe('203.0.113.5');
    expect(clientAddress('::1', '203.0.113.5')).toBe('203.0.113.5');
    expect(clientAddress('::ffff:127.0.0.1', '203.0.113.5')).toBe('203.0.113.5');
    expect(clientAddress('198.51.100.7', '203.0.113.5')).toBe('198.51.100.7');
    expect(clientAddress('::ffff:198.51.100.7', '203.0.113.5')).toBe('198.51.100.7');
    expect(clientAddress('10.0.0.1', '203.0.113.5')).toBe('10.0.0.1');
  });

  it('takes the entry added by the nearest proxy (the last), which the caller cannot choose', () => {
    expect(clientAddress('127.0.0.1', '1.1.1.1, 203.0.113.5')).toBe('203.0.113.5');
    expect(clientAddress('127.0.0.1', ['1.1.1.1', '2.2.2.2, 203.0.113.9'])).toBe('203.0.113.9');
    expect(clientAddress('127.0.0.1', ' 203.0.113.5 ')).toBe('203.0.113.5');
  });

  it('falls back to the connection address when the header is missing or not an address', () => {
    expect(clientAddress('127.0.0.1', undefined)).toBe('127.0.0.1');
    expect(clientAddress('127.0.0.1', '')).toBe('127.0.0.1');
    expect(clientAddress('127.0.0.1', 'not-an-ip')).toBe('127.0.0.1');
    expect(clientAddress('127.0.0.1', '203.0.113.5, junk')).toBe('127.0.0.1');
    expect(clientAddress(undefined, '203.0.113.5')).toBe('unknown');
  });
});

describe('CORS (R8)', () => {
  const ALLOWED = 'https://app.example';
  const CORS_HEADERS = [
    'access-control-allow-origin',
    'access-control-allow-headers',
    'access-control-allow-methods',
    'access-control-max-age',
    'access-control-allow-credentials',
  ];

  it('sends CORS headers to a listed origin, on answers and on errors', async () => {
    const r = await start({ allowedOrigins: [ALLOWED] });
    const ok = await call(r, 'GET', '/vault/v1/health', { headers: { Origin: ALLOWED } });
    expect(ok.headers['access-control-allow-origin']).toBe(ALLOWED);
    expect(ok.headers.vary).toContain('Origin');
    const err = await post(r, 'challenge', {}, { Origin: ALLOWED });
    expect(err.status).toBe(400);
    expect(err.headers['access-control-allow-origin']).toBe(ALLOWED);
  });

  it('answers a preflight from a listed origin with the allowed headers and methods', async () => {
    const r = await start({ allowedOrigins: [ALLOWED] });
    const res = await call(r, 'OPTIONS', '/vault/v1/replace', {
      headers: { Origin: ALLOWED, 'Access-Control-Request-Method': 'POST', 'Access-Control-Request-Headers': 'content-type' },
    });
    expect(res.status).toBe(204);
    expect(res.text).toBe('');
    expect(res.headers['access-control-allow-origin']).toBe(ALLOWED);
    expect(res.headers['access-control-allow-headers']).toBe('Content-Type, Authorization');
    expect(res.headers['access-control-allow-methods']).toBe('GET, POST, OPTIONS');
    expect(res.headers['access-control-allow-credentials']).toBeUndefined();
  });

  it('sends NO CORS header to an unlisted origin, on answers, errors and preflights', async () => {
    const r = await start({ allowedOrigins: [ALLOWED] });
    for (const origin of ['https://evil.example', 'https://app.example.evil.example', 'http://app.example', 'https://APP.example', 'null', `${ALLOWED}/`]) {
      const replies = [
        await call(r, 'GET', '/vault/v1/health', { headers: { Origin: origin } }),
        await post(r, 'challenge', {}, { Origin: origin }),
        await call(r, 'OPTIONS', '/vault/v1/replace', { headers: { Origin: origin, 'Access-Control-Request-Method': 'POST' } }),
      ];
      for (const res of replies) {
        for (const h of CORS_HEADERS) expect(res.headers[h]).toBeUndefined();
      }
      expect(replies[2]!.status).toBe(204);
    }
  });

  it('sends none at all with an empty list, and never a wildcard', async () => {
    const r = await start({ allowedOrigins: [] });
    const res = await call(r, 'GET', '/vault/v1/health', { headers: { Origin: ALLOWED } });
    const pre = await call(r, 'OPTIONS', '/vault/v1/open', { headers: { Origin: ALLOWED, 'Access-Control-Request-Method': 'POST' } });
    for (const h of CORS_HEADERS) {
      expect(res.headers[h]).toBeUndefined();
      expect(pre.headers[h]).toBeUndefined();
    }
  });

  it('sends none when there is no Origin header', async () => {
    const r = await start({ allowedOrigins: [ALLOWED] });
    const res = await call(r, 'GET', '/vault/v1/health');
    for (const h of CORS_HEADERS) expect(res.headers[h]).toBeUndefined();
  });
});
