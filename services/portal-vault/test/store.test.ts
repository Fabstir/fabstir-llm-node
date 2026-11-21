// R6: storage. Unit tests on the in-memory store, the record codec, the object
// keys and the per-account serialiser. The S3 test runs only when a test bucket
// is configured (VAULT_TEST_S3_*), and is skipped otherwise.
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  AccountLocks,
  CorruptRecordError,
  MemoryVaultStore,
  StorageTimeoutError,
  StorageUnsettledError,
  objectKey,
  parseRecord,
  serialiseRecord,
  storageFailureKind,
  type VaultRecord,
} from '../src/store.js';
import { S3VaultStore } from '../src/store-s3.js';

const A = '0x1234567890abcdef1234567890abcdef12345678';
const B = '0x1111111111111111111111111111111111111111';

function record(over: Partial<VaultRecord> = {}): VaultRecord {
  return { v: 1, address: A, chainId: 84532, version: 1, epoch: 1, updatedAt: '2026-11-21T06:00:00.000Z', vault: 'opaque-test-blob', ...over };
}

describe('objectKey', () => {
  it('follows the documented layout', () => {
    expect(objectKey(84532, A, 'current')).toBe(`v1/84532/${A}/current.json`);
    expect(objectKey(84532, A, 'previous')).toBe(`v1/84532/${A}/previous.json`);
  });

  it('refuses anything that is not a lower-case address and a chain id', () => {
    expect(() => objectKey(84532, A.toUpperCase(), 'current')).toThrow();
    expect(() => objectKey(84532, '../../etc', 'current')).toThrow();
    expect(() => objectKey(84532, `${A}/x`, 'current')).toThrow();
    expect(() => objectKey(-1, A, 'current')).toThrow();
    expect(() => objectKey(1.5, A, 'current')).toThrow();
    expect(() => objectKey(84532, A, 'other' as never)).toThrow();
  });
});

describe('record codec', () => {
  it('round trips with exactly the documented fields', () => {
    const text = serialiseRecord(record());
    expect(Object.keys(JSON.parse(text))).toEqual(['v', 'address', 'chainId', 'version', 'epoch', 'updatedAt', 'vault']);
    expect(parseRecord(text, 84532, A)).toEqual(record());
  });

  it('refuses a record stored under another account or chain', () => {
    expect(() => parseRecord(serialiseRecord(record()), 84532, B)).toThrow();
    expect(() => parseRecord(serialiseRecord(record()), 8453, A)).toThrow();
  });

  it('refuses malformed records', () => {
    expect(() => parseRecord('not json', 84532, A)).toThrow();
    expect(() => parseRecord('null', 84532, A)).toThrow();
    expect(() => parseRecord(JSON.stringify({ ...record(), v: 2 }), 84532, A)).toThrow();
    expect(() => parseRecord(JSON.stringify({ ...record(), version: 0 }), 84532, A)).toThrow();
    expect(() => parseRecord(JSON.stringify({ ...record(), epoch: '1' }), 84532, A)).toThrow();
    expect(() => parseRecord(JSON.stringify({ ...record(), vault: 5 }), 84532, A)).toThrow();
    expect(() => parseRecord(JSON.stringify({ ...record(), updatedAt: 5 }), 84532, A)).toThrow();
  });

  it('never puts the vault text in an error message', () => {
    const bad = JSON.stringify({ ...record({ vault: 'SECRET-VAULT-TEXT' }), version: 0 });
    try {
      parseRecord(bad, 84532, A);
      expect.unreachable();
    } catch (e) {
      expect(String((e as Error).message)).not.toContain('SECRET-VAULT-TEXT');
    }
  });
});

describe('MemoryVaultStore', () => {
  it('returns null when nothing is stored', async () => {
    const s = new MemoryVaultStore();
    expect(await s.readCurrent(84532, A)).toBeNull();
    expect(await s.readPrevious(84532, A)).toBeNull();
  });

  it('keeps current and previous apart, per account and per chain', async () => {
    const s = new MemoryVaultStore();
    await s.writeCurrent(record());
    await s.writePrevious(record({ version: 7 }));
    await s.writeCurrent(record({ address: B, version: 3 }));
    await s.writeCurrent(record({ chainId: 8453, version: 4 }));
    expect((await s.readCurrent(84532, A))!.version).toBe(1);
    expect((await s.readPrevious(84532, A))!.version).toBe(7);
    expect((await s.readCurrent(84532, B))!.version).toBe(3);
    expect((await s.readCurrent(8453, A))!.version).toBe(4);
    expect(await s.readPrevious(84532, B)).toBeNull();
  });

  it('hands out copies, not its own objects', async () => {
    const s = new MemoryVaultStore();
    await s.writeCurrent(record());
    const got = (await s.readCurrent(84532, A))!;
    got.version = 99;
    expect((await s.readCurrent(84532, A))!.version).toBe(1);
  });

  it('counts writes, pings ok, and can be made to fail', async () => {
    const s = new MemoryVaultStore();
    expect(await s.ping()).toBe(true);
    await s.writeCurrent(record());
    expect(s.writes).toEqual([`v1/84532/${A}/current.json`]);
    s.failing = true;
    expect(await s.ping()).toBe(false);
    await expect(s.readCurrent(84532, A)).rejects.toThrow();
    await expect(s.writeCurrent(record())).rejects.toThrow();
  });
});

describe('AccountLocks', () => {
  const tick = () => new Promise((r) => setTimeout(r, 5));

  it('runs work for one account one at a time, in order', async () => {
    const locks = new AccountLocks();
    const events: string[] = [];
    const job = (name: string) =>
      locks.run('k', async () => {
        events.push(`${name}:start`);
        await tick();
        events.push(`${name}:end`);
        return name;
      }, 1000);
    const results = await Promise.all([job('a'), job('b'), job('c')]);
    expect(results).toEqual(['a', 'b', 'c']);
    expect(events).toEqual(['a:start', 'a:end', 'b:start', 'b:end', 'c:start', 'c:end']);
  });

  it('lets different accounts run together', async () => {
    const locks = new AccountLocks();
    const events: string[] = [];
    const job = (key: string) =>
      locks.run(key, async () => {
        events.push(`${key}:start`);
        await tick();
        events.push(`${key}:end`);
      }, 1000);
    await Promise.all([job('x'), job('y')]);
    expect(events.slice(0, 2)).toEqual(['x:start', 'y:start']);
  });

  it('carries on after a failure and frees its memory when idle', async () => {
    const locks = new AccountLocks();
    await expect(locks.run('k', async () => { throw new Error('boom'); }, 1000)).rejects.toThrow('boom');
    expect(await locks.run('k', async () => 'after', 1000)).toBe('after');
    await tick(); // the entry is released just after the work settles
    expect(locks.size()).toBe(0);
  });
});

const env = process.env;
const s3Configured = Boolean(
  env.VAULT_TEST_S3_ENDPOINT && env.VAULT_TEST_S3_ACCESS_KEY && env.VAULT_TEST_S3_SECRET_KEY && env.VAULT_TEST_S3_BUCKET && env.VAULT_TEST_S3_REGION
);

describe.skipIf(!s3Configured)('S3VaultStore (integration; needs VAULT_TEST_S3_*)', () => {
  it('writes, reads back and reports missing objects as null', async () => {
    const s = new S3VaultStore({
      endpoint: env.VAULT_TEST_S3_ENDPOINT!,
      accessKey: env.VAULT_TEST_S3_ACCESS_KEY!,
      secretKey: env.VAULT_TEST_S3_SECRET_KEY!,
      bucket: env.VAULT_TEST_S3_BUCKET!,
      region: env.VAULT_TEST_S3_REGION!,
    });
    // A throwaway account so a shared test bucket is not disturbed.
    const address = '0x' + Date.now().toString(16).padStart(40, 'f');
    expect(await s.ping()).toBe(true);
    expect(await s.readCurrent(84532, address)).toBeNull();
    await s.writePrevious(record({ address, version: 1 }));
    await s.writeCurrent(record({ address, version: 2, epoch: 2 }));
    expect((await s.readPrevious(84532, address))!.version).toBe(1);
    expect(await s.readCurrent(84532, address)).toEqual(record({ address, version: 2, epoch: 2 }));
  });
});

describe('S3VaultStore construction', () => {
  it('refuses an endpoint that is not an http or https URL', () => {
    const base = { accessKey: 'a', secretKey: 'b', bucket: 'c', region: 'us-east-1' };
    expect(() => new S3VaultStore({ ...base, endpoint: 'ftp://x' })).toThrow();
    expect(() => new S3VaultStore({ ...base, endpoint: 'nonsense' })).toThrow();
    expect(() => new S3VaultStore({ ...base, endpoint: 'http://127.0.0.1:3900' })).not.toThrow();
  });
});

describe('AccountLocks with a timeout (R6a v0.2)', () => {
  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

  it('answers the caller at the timeout but HOLDS the lock until the work really settles', async () => {
    const locks = new AccountLocks();
    const events: string[] = [];
    const slow = locks.run(
      'k',
      async () => {
        events.push('slow:start');
        await sleep(80);
        events.push('slow:settled');
        return 'late';
      },
      20
    );
    await expect(slow).rejects.toBeInstanceOf(StorageTimeoutError);
    expect(events).toEqual(['slow:start']);
    expect(locks.isUnsettled('k')).toBe(true);

    // While it is unsettled nothing else for that account is accepted, and nothing runs.
    let ran = false;
    await expect(locks.run('k', async () => { ran = true; }, 20)).rejects.toBeInstanceOf(StorageUnsettledError);
    await expect(locks.run('k', async () => { ran = true; }, 5000)).rejects.toBeInstanceOf(StorageUnsettledError);
    expect(ran).toBe(false);
    // Another account is not affected.
    expect(await locks.run('other', async () => 'fine', 20)).toBe('fine');

    await sleep(90);
    expect(events).toEqual(['slow:start', 'slow:settled']);
    expect(locks.isUnsettled('k')).toBe(false);
    expect(await locks.run('k', async () => 'after', 20)).toBe('after');
  });

  it('releases the lock when the late work fails, too', async () => {
    const locks = new AccountLocks();
    const slow = locks.run('k', async () => { await sleep(40); throw new Error('late failure'); }, 10);
    await expect(slow).rejects.toBeInstanceOf(StorageTimeoutError);
    await sleep(50);
    expect(locks.isUnsettled('k')).toBe(false);
    expect(await locks.run('k', async () => 'after', 20)).toBe('after');
    await sleep(5);
    expect(locks.size()).toBe(0);
  });

  it('work that timed out while still queued never starts', async () => {
    const locks = new AccountLocks();
    let started = false;
    const first = locks.run('k', async () => { await sleep(60); return 1; }, 1000);
    const queued = locks.run('k', async () => { started = true; return 2; }, 15);
    await expect(queued).rejects.toBeInstanceOf(StorageTimeoutError);
    expect(await first).toBe(1);
    await sleep(10);
    expect(started).toBe(false);
    expect(locks.isUnsettled('k')).toBe(false);
  });

  it('has no "no timeout": a missing or nonsense timeout is refused and the work is not run (round 2)', async () => {
    const locks = new AccountLocks();
    let ran = false;
    const work = async () => {
      ran = true;
    };
    for (const bad of [undefined, null, 0, -5, NaN, Infinity, '10']) {
      await expect((async () => locks.run('k', work, bad as never))()).rejects.toThrow(/timeout/);
    }
    expect(ran).toBe(false);
    expect(locks.size()).toBe(0);
    expect(await locks.run('k', async () => 'ok', 50)).toBe('ok');
  });

  it('tells the work when its caller has gone, so it can stop before writing', async () => {
    const locks = new AccountLocks();
    let seen: boolean | undefined;
    const p = locks.run('k', async (ctx) => { await sleep(40); seen = ctx.abandoned; }, 10);
    await expect(p).rejects.toBeInstanceOf(StorageTimeoutError);
    await sleep(50);
    expect(seen).toBe(true);
    let fast: boolean | undefined;
    await locks.run('k', async (ctx) => { fast = ctx.abandoned; }, 50);
    expect(fast).toBe(false);
  });
});

describe('storageFailureKind (R9 v0.2)', () => {
  it('names the kind and never passes a message on', () => {
    expect(storageFailureKind(new StorageTimeoutError())).toBe('timeout');
    expect(storageFailureKind(new StorageUnsettledError())).toBe('unsettled');
    expect(storageFailureKind(new CorruptRecordError('x'))).toBe('corrupt');
    expect(storageFailureKind(Object.assign(new Error('secret message text'), { code: 'AccessDenied' }))).toBe('AccessDenied');
    expect(storageFailureKind(Object.assign(new Error('m'), { code: 'ECONNREFUSED' }))).toBe('ECONNREFUSED');
    expect(storageFailureKind(new Error('secret message text'))).toBe('error');
    expect(storageFailureKind(Object.assign(new Error('m'), { code: 'has spaces and <xml>' }))).toBe('error');
    expect(storageFailureKind(Object.assign(new Error('m'), { code: 'x'.repeat(100) }))).toBe('error');
    expect(storageFailureKind(Object.assign(new Error('m'), { code: 500 }))).toBe('error');
    expect(storageFailureKind(null)).toBe('error');
  });

  it('a malformed stored record is a CorruptRecordError', () => {
    expect(() => parseRecord('not json', 84532, A)).toThrow(CorruptRecordError);
    expect(() => parseRecord(JSON.stringify({ ...record(), version: 0 }), 84532, A)).toThrow(CorruptRecordError);
    expect(() => parseRecord(serialiseRecord(record()), 84532, B)).toThrow(CorruptRecordError);
  });
});

describe('S3VaultStore against a stub HTTP server: what counts as "no vault" (R6b v0.2)', () => {
  type Mode = { status: number; body: string; contentType?: string };
  const mode: { v: Mode } = { v: { status: 404, body: '' } };
  let server: http.Server | undefined;
  const xml = (code: string) => `<?xml version="1.0" encoding="UTF-8"?><Error><Code>${code}</Code><Message>stub message</Message></Error>`;

  async function stubStore(): Promise<S3VaultStore> {
    server = http.createServer((req, res) => {
      req.resume();
      res.writeHead(mode.v.status, mode.v.contentType ? { 'content-type': mode.v.contentType } : {});
      res.end(mode.v.body);
    });
    await new Promise<void>((r) => server!.listen(0, '127.0.0.1', r));
    const port = (server.address() as AddressInfo).port;
    return new S3VaultStore({ endpoint: `http://127.0.0.1:${port}`, accessKey: 'a', secretKey: 'b', bucket: 'vault-bucket', region: 'us-east-1' });
  }

  afterEach(async () => {
    vi.restoreAllMocks();
    if (server) {
      server.closeAllConnections();
      await new Promise((r) => server!.close(r));
      server = undefined;
    }
  });

  it('returns null ONLY for HTTP 404 with the S3 code NoSuchKey', async () => {
    const s = await stubStore();
    mode.v = { status: 404, body: xml('NoSuchKey'), contentType: 'application/xml' };
    expect(await s.readCurrent(84532, A)).toBeNull();
    expect(await s.readPrevious(84532, A)).toBeNull();
  });

  it('throws for every other answer: an outage is never mistaken for an empty account', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => {}); // the S3 client announces its one retry of a 5xx
    const s = await stubStore();
    const others: Array<[string, Mode]> = [
      ['empty 404', { status: 404, body: '' }],
      ['HTML 404', { status: 404, body: '<html>nope</html>', contentType: 'text/html' }],
      ['NoSuchBucket', { status: 404, body: xml('NoSuchBucket'), contentType: 'application/xml' }],
      ['NotFound in XML', { status: 404, body: xml('NotFound'), contentType: 'application/xml' }],
      ['403', { status: 403, body: xml('AccessDenied'), contentType: 'application/xml' }],
      ['empty 403', { status: 403, body: '' }],
      ['500', { status: 500, body: xml('InternalError'), contentType: 'application/xml' }],
      ['NoSuchKey with a status that is not 404', { status: 400, body: xml('NoSuchKey'), contentType: 'application/xml' }],
      ['NoSuchKey with status 403', { status: 403, body: xml('NoSuchKey'), contentType: 'application/xml' }],
    ];
    for (const [name, m] of others) {
      mode.v = m;
      let outcome: unknown = 'no error';
      try {
        outcome = await s.readCurrent(84532, A);
      } catch (e) {
        outcome = e;
      }
      expect(outcome, name).toBeInstanceOf(Error);
    }
  });

  it('reports the S3 code as the failure kind', async () => {
    const s = await stubStore();
    mode.v = { status: 404, body: xml('NoSuchBucket'), contentType: 'application/xml' };
    const err = await s.readCurrent(84532, A).catch((e) => e);
    expect(storageFailureKind(err)).toBe('NoSuchBucket');
    mode.v = { status: 403, body: '' };
    expect(storageFailureKind(await s.readCurrent(84532, A).catch((e) => e))).toBe('AccessDenied');
  });

  it('a stored body that is not a record is corrupt, not "no vault"', async () => {
    const s = await stubStore();
    mode.v = { status: 200, body: '{"half":' };
    const err = await s.readCurrent(84532, A).catch((e) => e);
    expect(err).toBeInstanceOf(CorruptRecordError);
  });

  it('keeps concurrent reads apart: one account missing, another failing', async () => {
    const s = await stubStore();
    server!.removeAllListeners('request');
    server!.on('request', (req: http.IncomingMessage, res: http.ServerResponse) => {
      req.resume();
      const missing = req.url!.includes(A);
      setTimeout(() => {
        res.writeHead(missing ? 404 : 403, { 'content-type': 'application/xml' });
        res.end(missing ? xml('NoSuchKey') : xml('NoSuchKey'));
      }, missing ? 30 : 5);
    });
    const [a, b] = await Promise.all([s.readCurrent(84532, A).catch((e) => e), s.readCurrent(84532, B).catch((e) => e)]);
    expect(a).toBeNull();
    expect(b).toBeInstanceOf(Error);
  });
});
