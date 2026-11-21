// Spec section 8, handlers: every case, over the in-memory store, a fake
// verifier and an injected clock.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { handleChallenge, handleCurrent, handleHealth, handleOpen, handleReplace, type HandlerResult } from '../src/handlers.js';
import { withConcurrencyLimit } from '../src/signature.js';
import { ADDR_A, ADDR_A_LC, ADDR_B, CHAIN, fakeSign, makeEnv, sha256Hex, type TestEnv } from './helpers.js';

const VAULT_1 = 'wrapped-vault-one:' + 'a'.repeat(200);
const VAULT_2 = 'wrapped-vault-two:' + 'b'.repeat(200);

type Body = Record<string, any>;
const body = (r: HandlerResult) => r.body as Body;

async function challengeFor(env: TestEnv, purpose: 'read' | 'replace', extra: Body = {}, address = ADDR_A): Promise<Body> {
  const r = await handleChallenge(env.deps, { address, chainId: CHAIN, purpose, ...extra });
  expect(r.status).toBe(200);
  return body(r);
}

async function open(env: TestEnv, opts: { address?: string; form?: 'message' | 'typedData' } = {}): Promise<HandlerResult> {
  const address = opts.address ?? ADDR_A;
  const form = opts.form ?? 'message';
  const c = await challengeFor(env, 'read', {}, address);
  const signature = fakeSign(address, form === 'message' ? { message: c.message } : { typedData: c.typedData });
  return handleOpen(env.deps, { address, chainId: CHAIN, nonce: c.nonce, signature, form });
}

async function replace(
  env: TestEnv,
  vault: string,
  replaces: number,
  opts: { address?: string; form?: 'message' | 'typedData' } = {}
): Promise<HandlerResult> {
  const address = opts.address ?? ADDR_A;
  const form = opts.form ?? 'message';
  const c = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(vault), replaces }, address);
  const signature = fakeSign(address, form === 'message' ? { message: c.message } : { typedData: c.typedData });
  return handleReplace(env.deps, { address, chainId: CHAIN, nonce: c.nonce, signature, form, vault, replaces });
}

const current = (env: TestEnv, token: string | undefined, address = ADDR_A, chainId: string = String(CHAIN)) =>
  handleCurrent(env.deps, { address, chainId, authorization: token === undefined ? undefined : `Bearer ${token}` });

afterEach(() => vi.restoreAllMocks());

describe('challenge', () => {
  it('returns every field of a read challenge, and a message built from them', async () => {
    const env = makeEnv();
    const c = await challengeFor(env, 'read');
    expect(Object.keys(c).sort()).toEqual(
      ['address', 'chainId', 'expires', 'message', 'nonce', 'portal', 'purpose', 'region', 'typedData'].sort()
    );
    expect(c.purpose).toBe('read');
    expect(c.region).toBe('uk');
    expect(c.portal).toBe('uk.portal.example');
    expect(c.address).toBe(ADDR_A_LC);
    expect(c.chainId).toBe(CHAIN);
    expect(c.nonce).toMatch(/^[0-9a-f]{48}$/);
    expect(c.expires).toBe('2026-11-21T06:05:00.000Z');
    expect(c.message).toBe(
      'Platformless AI: open your key vault\n' +
        'region: uk\n' +
        'portal: uk.portal.example\n' +
        `address: ${ADDR_A_LC}\n` +
        'chain: 84532\n' +
        `nonce: ${c.nonce}\n` +
        'expires: 2026-11-21T06:05:00.000Z'
    );
    expect(c.typedData.primaryType).toBe('VaultRead');
    expect(c.typedData.domain).toEqual({ name: 'Platformless AI Vault', version: '1', chainId: CHAIN });
    expect(c.typedData.message).toEqual({
      purpose: 'open your key vault',
      region: 'uk',
      portal: 'uk.portal.example',
      wallet: ADDR_A_LC,
      nonce: c.nonce,
      expires: c.expires,
    });
  });

  it('returns the hash and the version of a replace challenge and binds them into the message', async () => {
    const env = makeEnv();
    const hash = sha256Hex(VAULT_1);
    const c = await challengeFor(env, 'replace', { vaultSha256: hash, replaces: 4 });
    expect(c.vaultSha256).toBe(hash);
    expect(c.replaces).toBe(4);
    expect(c.message).toBe(
      'Platformless AI: replace your key vault\n' +
        'region: uk\n' +
        'portal: uk.portal.example\n' +
        `address: ${ADDR_A_LC}\n` +
        'chain: 84532\n' +
        `vault: ${hash}\n` +
        'replaces: 4\n' +
        `nonce: ${c.nonce}\n` +
        'expires: 2026-11-21T06:05:00.000Z'
    );
    expect(c.typedData.primaryType).toBe('VaultReplace');
    expect(c.typedData.message.vault).toBe(hash);
    expect(c.typedData.message.replaces).toBe('4');
  });

  it('is idempotent within the life of the nonce, and does not store the hash or the version', async () => {
    const env = makeEnv();
    const first = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(VAULT_1), replaces: 0 });
    env.clock.t += 120_000;
    const second = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(VAULT_2), replaces: 7 });
    expect(second.nonce).toBe(first.nonce);
    expect(second.expires).toBe(first.expires);
    expect(second.vaultSha256).toBe(sha256Hex(VAULT_2));
    expect(second.replaces).toBe(7);
    expect(second.message).not.toBe(first.message);
    expect(second.message).toContain(`vault: ${sha256Hex(VAULT_2)}\nreplaces: 7\n`);
    const read = await challengeFor(env, 'read');
    expect(read.nonce).not.toBe(first.nonce);
  });

  it('issues a fresh nonce once the old one has expired', async () => {
    const env = makeEnv();
    const first = await challengeFor(env, 'read');
    env.clock.t += 300_000;
    expect((await challengeFor(env, 'read')).nonce).not.toBe(first.nonce);
  });

  it('refuses malformed requests with 400 invalid-request', async () => {
    const env = makeEnv();
    const hash = sha256Hex(VAULT_1);
    const bad: unknown[] = [
      null,
      'text',
      [],
      {},
      { address: 'nope', chainId: CHAIN, purpose: 'read' },
      { address: ADDR_A, chainId: String(CHAIN), purpose: 'read' },
      { address: ADDR_A, chainId: 1.5, purpose: 'read' },
      { address: ADDR_A, chainId: CHAIN },
      { address: ADDR_A, chainId: CHAIN, purpose: 'delete' },
      { address: ADDR_A, chainId: CHAIN, purpose: 'constructor' },
      { address: ADDR_A, chainId: CHAIN, purpose: 'read', vaultSha256: hash },
      { address: ADDR_A, chainId: CHAIN, purpose: 'read', replaces: 0 },
      { address: ADDR_A, chainId: CHAIN, purpose: 'replace' },
      { address: ADDR_A, chainId: CHAIN, purpose: 'replace', vaultSha256: hash },
      { address: ADDR_A, chainId: CHAIN, purpose: 'replace', replaces: 0 },
      { address: ADDR_A, chainId: CHAIN, purpose: 'replace', vaultSha256: hash.toUpperCase(), replaces: 0 },
      { address: ADDR_A, chainId: CHAIN, purpose: 'replace', vaultSha256: hash, replaces: -1 },
      { address: ADDR_A, chainId: CHAIN, purpose: 'replace', vaultSha256: hash, replaces: 1.5 },
      { address: ADDR_A, chainId: CHAIN, purpose: 'replace', vaultSha256: hash, replaces: '0' },
    ];
    for (const b of bad) {
      const r = await handleChallenge(env.deps, b);
      expect(r.status).toBe(400);
      expect(r.body).toEqual({ error: 'invalid-request' });
    }
    expect(env.challenges.size()).toBe(0);
  });

  it('answers 429 too-many-challenges past the global cap', async () => {
    const env = makeEnv({ challengeMax: 1 });
    await challengeFor(env, 'read');
    const r = await handleChallenge(env.deps, { address: ADDR_B, chainId: CHAIN, purpose: 'read' });
    expect(r.status).toBe(429);
    expect(r.body).toEqual({ error: 'too-many-challenges' });
    expect(r.headers?.['Retry-After']).toBeDefined();
  });

  it('reveals nothing about whether a vault exists', async () => {
    const env = makeEnv();
    expect((await replace(env, VAULT_1, 0)).status).toBe(200);
    const withVault = await challengeFor(env, 'read');
    const without = await challengeFor(env, 'read', {}, ADDR_B);
    expect(Object.keys(withVault).sort()).toEqual(Object.keys(without).sort());
    expect(env.store.writes).toHaveLength(1); // the challenge call itself never touches storage
  });
});

describe('unsupported chain (R4)', () => {
  it('refuses any chain but the configured one on every call, before anything else happens', async () => {
    const env = makeEnv();
    const c = await challengeFor(env, 'read');
    const sig = fakeSign(ADDR_A, { message: c.message });
    const results = [
      await handleChallenge(env.deps, { address: ADDR_A, chainId: 8453, purpose: 'read' }),
      await handleOpen(env.deps, { address: ADDR_A, chainId: 8453, nonce: c.nonce, signature: sig, form: 'message' }),
      await handleReplace(env.deps, { address: ADDR_A, chainId: 8453, nonce: c.nonce, signature: sig, form: 'message', vault: VAULT_1, replaces: 0 }),
      await handleCurrent(env.deps, { address: ADDR_A, chainId: '8453', authorization: 'Bearer x.y' }),
    ];
    for (const r of results) {
      expect(r.status).toBe(400);
      expect(r.body).toEqual({ error: 'unsupported-chain' });
    }
    expect(env.verifier.calls).toHaveLength(0);
    expect(env.challenges.size()).toBe(1);
    expect(env.store.writes).toHaveLength(0);
  });
});

describe('open', () => {
  it('answers 404 no-vault when the signature is valid and nothing is stored, and consumes the nonce', async () => {
    const env = makeEnv();
    const c = await challengeFor(env, 'read');
    const request = { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_A, { message: c.message }), form: 'message' };
    const r = await handleOpen(env.deps, request);
    expect(r.status).toBe(404);
    expect(r.body).toEqual({ error: 'no-vault' });
    const again = await handleOpen(env.deps, request);
    expect(again.status).toBe(401);
    expect(again.body).toEqual({ error: 'challenge-unknown' });
  });

  it('returns the vault and a read token when one is stored', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    env.clock.t += 1000;
    const r = await open(env);
    expect(r.status).toBe(200);
    expect(Object.keys(r.body).sort()).toEqual(['readToken', 'readTokenExpires', 'updatedAt', 'vault', 'version'].sort());
    expect(body(r).version).toBe(1);
    expect(body(r).vault).toBe(VAULT_1);
    expect(body(r).updatedAt).toBe('2026-11-21T06:00:00.000Z');
    expect(body(r).readTokenExpires).toBe(new Date(env.clock.t + 30 * 24 * 3600 * 1000).toISOString());
    expect((await current(env, body(r).readToken)).status).toBe(200);
  });

  it('accepts the typed-data form, with the stored challenge rebuilt by the service', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    const r = await open(env, { form: 'typedData' });
    expect(r.status).toBe(200);
    const proof = env.verifier.calls.at(-1)!;
    expect('typedData' in proof).toBe(true);
  });

  it('accepts any casing of the address in the request', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    const c = await challengeFor(env, 'read', {}, ADDR_A_LC);
    const r = await handleOpen(env.deps, {
      address: '0x' + ADDR_A.slice(2).toUpperCase(),
      chainId: CHAIN,
      nonce: c.nonce,
      signature: fakeSign(ADDR_A, { message: c.message }),
      form: 'message',
    });
    expect(r.status).toBe(200);
  });

  it('does NOT consume the nonce on a failed verification (R2): the owner can still open', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    const c = await challengeFor(env, 'read');
    for (let i = 0; i < 5; i++) {
      const r = await handleOpen(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: '0x' + 'ee'.repeat(65), form: 'message' });
      expect(r.status).toBe(401);
      expect(r.body).toEqual({ error: 'signature-invalid' });
    }
    expect((await challengeFor(env, 'read')).nonce).toBe(c.nonce);
    const ok = await handleOpen(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_A, { message: c.message }), form: 'message' });
    expect(ok.status).toBe(200);
  });

  it('answers 503 with Retry-After when the chain cannot be asked, never "invalid", and keeps the nonce', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    const c = await challengeFor(env, 'read');
    const request = { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_A, { message: c.message }), form: 'message' };
    env.verifier.mode = 'unavailable';
    const r = await handleOpen(env.deps, request);
    expect(r.status).toBe(503);
    expect(r.body).toEqual({ error: 'signature-check-unavailable' });
    expect(Number(r.headers?.['Retry-After'])).toBeGreaterThan(0);
    env.verifier.mode = 'strict';
    expect((await handleOpen(env.deps, request)).status).toBe(200);
  });

  it('consumes the nonce on a verified request: a replay is refused', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    const c = await challengeFor(env, 'read');
    const request = { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_A, { message: c.message }), form: 'message' };
    expect((await handleOpen(env.deps, request)).status).toBe(200);
    const replay = await handleOpen(env.deps, request);
    expect(replay.status).toBe(401);
    expect(replay.body).toEqual({ error: 'challenge-unknown' });
    expect((await challengeFor(env, 'read')).nonce).not.toBe(c.nonce);
  });

  it('lets exactly one of two racing verified requests through on one nonce', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    const c = await challengeFor(env, 'read');
    const request = { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_A, { message: c.message }), form: 'message' };
    env.verifier.delayMs = 10;
    const results = await Promise.all([handleOpen(env.deps, request), handleOpen(env.deps, request)]);
    expect(results.map((r) => r.status).sort()).toEqual([200, 401]);
  });

  it('answers 401 challenge-unknown for an unknown or expired nonce, without asking the chain', async () => {
    const env = makeEnv();
    const unknown = await handleOpen(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: 'ab'.repeat(24), signature: '0x' + 'ee'.repeat(65), form: 'message' });
    expect(unknown.status).toBe(401);
    expect(unknown.body).toEqual({ error: 'challenge-unknown' });

    const c = await challengeFor(env, 'read');
    env.clock.t += 300_000;
    const expired = await handleOpen(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_A, { message: c.message }), form: 'message' });
    expect(expired.status).toBe(401);
    expect(expired.body).toEqual({ error: 'challenge-unknown' });
    expect(env.verifier.calls).toHaveLength(0);
  });

  it('refuses a nonce issued for another address, even if the verifier would say yes, and leaves it live', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    const forA = await challengeFor(env, 'read');
    env.verifier.mode = 'always-true';
    const calls = env.verifier.calls.length;
    const r = await handleOpen(env.deps, { address: ADDR_B, chainId: CHAIN, nonce: forA.nonce, signature: '0x' + 'ee'.repeat(65), form: 'message' });
    expect(r.status).toBe(401);
    expect(r.body).toEqual({ error: 'challenge-unknown' });
    expect(env.verifier.calls).toHaveLength(calls);
    expect(env.challenges.peek(forA.nonce)).not.toBeNull();
  });

  it('refuses a replace nonce offered to open, even if the verifier would say yes', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    const c = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(VAULT_2), replaces: 1 });
    env.verifier.mode = 'always-true';
    const r = await handleOpen(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: '0x' + 'ee'.repeat(65), form: 'message' });
    expect(r.status).toBe(401);
    expect(r.body).toEqual({ error: 'challenge-unknown' });
    expect(env.challenges.peek(c.nonce)).not.toBeNull();
  });

  it('refuses a signature made in one form and offered as the other', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    const c = await challengeFor(env, 'read');
    const r = await handleOpen(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_A, { message: c.message }), form: 'typedData' });
    expect(r.status).toBe(401);
    expect(r.body).toEqual({ error: 'signature-invalid' });
  });

  it("refuses another wallet's signature over the owner's challenge", async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    const c = await challengeFor(env, 'read');
    const r = await handleOpen(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_B, { message: c.message }), form: 'message' });
    expect(r.status).toBe(401);
    expect(r.body).toEqual({ error: 'signature-invalid' });
  });

  it('refuses malformed requests with 400, before the chain is asked', async () => {
    const env = makeEnv();
    const c = await challengeFor(env, 'read');
    const good = { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_A, { message: c.message }), form: 'message' };
    const bad: unknown[] = [
      null,
      [],
      { ...good, address: 'nope' },
      { ...good, chainId: '84532' },
      { ...good, nonce: 'short' },
      { ...good, nonce: c.nonce.toUpperCase() },
      { ...good, nonce: 5 },
      { ...good, signature: 'not-hex' },
      { ...good, signature: '0xabc' },
      { ...good, signature: undefined },
      { ...good, form: 'raw' },
      { ...good, form: undefined },
    ];
    for (const b of bad) {
      const r = await handleOpen(env.deps, b);
      expect(r.status).toBe(400);
      expect(r.body).toEqual({ error: 'invalid-request' });
    }
    expect(env.verifier.calls).toHaveLength(0);
    expect(env.challenges.peek(c.nonce)).not.toBeNull();
  });

  it('answers 503 storage-unavailable when the storage fails', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    env.store.failing = true;
    const r = await open(env);
    expect(r.status).toBe(503);
    expect(r.body).toEqual({ error: 'storage-unavailable' });
    expect(r.headers?.['Retry-After']).toBeDefined();
  });
});

describe('replace', () => {
  it('creates version 1 from 0 and returns a working read token', async () => {
    const env = makeEnv();
    const r = await replace(env, VAULT_1, 0);
    expect(r.status).toBe(200);
    expect(Object.keys(r.body).sort()).toEqual(['readToken', 'readTokenExpires', 'updatedAt', 'version'].sort());
    expect(body(r).version).toBe(1);
    expect(body(r).updatedAt).toBe('2026-11-21T06:00:00.000Z');
    const stored = (await env.store.readCurrent(CHAIN, ADDR_A_LC))!;
    expect(stored).toMatchObject({ v: 1, address: ADDR_A_LC, chainId: CHAIN, version: 1, vault: VAULT_1 });
    const got = await current(env, body(r).readToken);
    expect(got.status).toBe(200);
    expect(got.body).toEqual({ version: 1, updatedAt: '2026-11-21T06:00:00.000Z', vault: VAULT_1 });
  });

  it('accepts the typed-data form', async () => {
    const env = makeEnv();
    expect((await replace(env, VAULT_1, 0, { form: 'typedData' })).status).toBe(200);
  });

  it('answers 409 version-conflict with the current version when "replaces" is wrong, and writes nothing', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    const writes = env.store.writes.length;

    const stale = await replace(env, VAULT_2, 0);
    expect(stale.status).toBe(409);
    expect(stale.body).toEqual({ error: 'version-conflict', currentVersion: 1 });

    const ahead = await replace(env, VAULT_2, 5);
    expect(ahead.status).toBe(409);
    expect(ahead.body).toEqual({ error: 'version-conflict', currentVersion: 1 });

    const none = await replace(env, VAULT_2, 1, { address: ADDR_B });
    expect(none.status).toBe(409);
    expect(none.body).toEqual({ error: 'version-conflict', currentVersion: 0 });

    expect(env.store.writes).toHaveLength(writes);
    expect((await env.store.readCurrent(CHAIN, ADDR_A_LC))!.vault).toBe(VAULT_1);
  });

  it('consumes the nonce on a conflict (the signature verified)', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    const c = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(VAULT_2), replaces: 0 });
    const request = { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_A, { message: c.message }), form: 'message', vault: VAULT_2, replaces: 0 };
    expect((await handleReplace(env.deps, request)).status).toBe(409);
    const again = await handleReplace(env.deps, request);
    expect(again.status).toBe(401);
    expect(again.body).toEqual({ error: 'challenge-unknown' });
  });

  it('refuses a signature that covers another hash: the service hashes the text it received (401, nothing written)', async () => {
    const env = makeEnv();
    const c = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(VAULT_1), replaces: 0 });
    const signature = fakeSign(ADDR_A, { message: c.message }); // signed for VAULT_1
    const r = await handleReplace(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature, form: 'message', vault: VAULT_2, replaces: 0 });
    expect(r.status).toBe(401);
    expect(r.body).toEqual({ error: 'signature-invalid' });
    expect(env.store.writes).toHaveLength(0);
    expect(await env.store.readCurrent(CHAIN, ADDR_A_LC)).toBeNull();

    // What was put to the verifier carried the hash of the RECEIVED text.
    const proof = env.verifier.calls.at(-1)!;
    expect('message' in proof && proof.message).toContain(`\nvault: ${sha256Hex(VAULT_2)}\n`);
    expect('message' in proof && proof.message).not.toContain(sha256Hex(VAULT_1));
  });

  it('ignores a hash supplied by the caller in the replace request', async () => {
    const env = makeEnv();
    const c = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(VAULT_1), replaces: 0 });
    const signature = fakeSign(ADDR_A, { message: c.message });
    const r = await handleReplace(env.deps, {
      address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature, form: 'message', vault: VAULT_2, replaces: 0,
      vaultSha256: sha256Hex(VAULT_1),
    });
    expect(r.status).toBe(401);
    expect(env.store.writes).toHaveLength(0);
  });

  it('binds the typed-data form to the hash of the received text too', async () => {
    const env = makeEnv();
    const c = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(VAULT_1), replaces: 0 });
    const signature = fakeSign(ADDR_A, { typedData: c.typedData });
    const r = await handleReplace(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature, form: 'typedData', vault: VAULT_2, replaces: 0 });
    expect(r.status).toBe(401);
    const proof = env.verifier.calls.at(-1)!;
    expect('typedData' in proof && proof.typedData.message.vault).toBe(sha256Hex(VAULT_2));
    expect(env.store.writes).toHaveLength(0);
  });

  it('refuses a signature that covers another "replaces" (401, nothing written)', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    const writes = env.store.writes.length;
    const c = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(VAULT_2), replaces: 0 });
    const signature = fakeSign(ADDR_A, { message: c.message }); // signed for replaces 0
    const r = await handleReplace(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature, form: 'message', vault: VAULT_2, replaces: 1 });
    expect(r.status).toBe(401);
    expect(r.body).toEqual({ error: 'signature-invalid' });
    expect(env.store.writes).toHaveLength(writes);
  });

  it('refuses a read signature offered to replace (401 challenge-unknown, nothing written, read nonce still live)', async () => {
    const env = makeEnv();
    const c = await challengeFor(env, 'read');
    const signature = fakeSign(ADDR_A, { message: c.message });
    const r = await handleReplace(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature, form: 'message', vault: VAULT_1, replaces: 0 });
    expect(r.status).toBe(401);
    expect(r.body).toEqual({ error: 'challenge-unknown' });
    expect(env.verifier.calls).toHaveLength(0);
    expect(env.store.writes).toHaveLength(0);
    expect(env.challenges.peek(c.nonce)).not.toBeNull();
  });

  it('refuses a read nonce for replace even if the verifier would say yes (R3 does not rest on the signature alone)', async () => {
    const env = makeEnv();
    const c = await challengeFor(env, 'read');
    env.verifier.mode = 'always-true';
    const r = await handleReplace(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: '0x' + 'ee'.repeat(65), form: 'message', vault: VAULT_1, replaces: 0 });
    expect(r.status).toBe(401);
    expect(env.store.writes).toHaveLength(0);
    expect(await env.store.readCurrent(CHAIN, ADDR_A_LC)).toBeNull();
  });

  it('refuses a replace nonce issued for another address, even if the verifier would say yes', async () => {
    const env = makeEnv();
    const forA = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(VAULT_1), replaces: 0 });
    env.verifier.mode = 'always-true';
    const r = await handleReplace(env.deps, { address: ADDR_B, chainId: CHAIN, nonce: forA.nonce, signature: '0x' + 'ee'.repeat(65), form: 'message', vault: VAULT_1, replaces: 0 });
    expect(r.status).toBe(401);
    expect(r.body).toEqual({ error: 'challenge-unknown' });
    expect(env.store.writes).toHaveLength(0);
  });

  it('answers 413 vault-too-large over the limit, counted in UTF-8 bytes, before the chain is asked', async () => {
    const env = makeEnv({ config: { maxVaultBytes: 100 } });
    const atLimit = 'x'.repeat(100);
    expect((await replace(env, atLimit, 0)).status).toBe(200);

    const calls = env.verifier.calls.length;
    const writes = env.store.writes.length;
    for (const tooBig of ['x'.repeat(101), 'é'.repeat(51)]) {
      const c = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(tooBig), replaces: 1 });
      const r = await handleReplace(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_A, { message: c.message }), form: 'message', vault: tooBig, replaces: 1 });
      expect(r.status).toBe(413);
      expect(r.body).toEqual({ error: 'vault-too-large' });
    }
    expect(env.verifier.calls).toHaveLength(calls);
    expect(env.store.writes).toHaveLength(writes);
  });

  it('keeps the previous version', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    expect(await env.store.readPrevious(CHAIN, ADDR_A_LC)).toBeNull();
    env.clock.t += 5000;
    const r = await replace(env, VAULT_2, 1);
    expect(r.status).toBe(200);
    expect(body(r).version).toBe(2);
    const previous = (await env.store.readPrevious(CHAIN, ADDR_A_LC))!;
    expect(previous.version).toBe(1);
    expect(previous.vault).toBe(VAULT_1);
    const now = (await env.store.readCurrent(CHAIN, ADDR_A_LC))!;
    expect(now.version).toBe(2);
    expect(now.vault).toBe(VAULT_2);
    expect(now.epoch).toBeGreaterThan(previous.epoch);
    // previous.json is written before current.json
    expect(env.store.writes.slice(-2)).toEqual([`v1/${CHAIN}/${ADDR_A_LC}/previous.json`, `v1/${CHAIN}/${ADDR_A_LC}/current.json`]);
  });

  it('revokes every read token issued before it, and its own new token works', async () => {
    const env = makeEnv();
    const first = await replace(env, VAULT_1, 0);
    const opened = await open(env);
    const oldTokens = [body(first).readToken, body(opened).readToken];
    for (const t of oldTokens) expect((await current(env, t)).status).toBe(200);

    const second = await replace(env, VAULT_2, 1);
    for (const t of oldTokens) {
      const r = await current(env, t);
      expect(r.status).toBe(401);
      expect(r.body).toEqual({ error: 'token-invalid' });
    }
    const fresh = await current(env, body(second).readToken);
    expect(fresh.status).toBe(200);
    expect(body(fresh).vault).toBe(VAULT_2);
  });

  it('does not consume the nonce on a failed or unavailable verification (R2)', async () => {
    const env = makeEnv();
    const c = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(VAULT_1), replaces: 0 });
    const good = { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_A, { message: c.message }), form: 'message', vault: VAULT_1, replaces: 0 };
    const bad = await handleReplace(env.deps, { ...good, signature: '0x' + 'ee'.repeat(65) });
    expect(bad.status).toBe(401);
    expect(bad.body).toEqual({ error: 'signature-invalid' });
    env.verifier.mode = 'unavailable';
    const down = await handleReplace(env.deps, good);
    expect(down.status).toBe(503);
    expect(down.body).toEqual({ error: 'signature-check-unavailable' });
    expect(down.headers?.['Retry-After']).toBeDefined();
    expect(env.store.writes).toHaveLength(0);
    env.verifier.mode = 'strict';
    expect((await handleReplace(env.deps, good)).status).toBe(200);
  });

  it('lets exactly one of two racing replaces through on one nonce', async () => {
    const env = makeEnv();
    const c = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(VAULT_1), replaces: 0 });
    const request = { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_A, { message: c.message }), form: 'message', vault: VAULT_1, replaces: 0 };
    env.verifier.delayMs = 10;
    const results = await Promise.all([handleReplace(env.deps, request), handleReplace(env.deps, request)]);
    expect(results.map((r) => r.status).sort()).toEqual([200, 401]);
    expect(env.store.writes).toHaveLength(1);
  });

  it('refuses malformed requests with 400', async () => {
    const env = makeEnv();
    const c = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(VAULT_1), replaces: 0 });
    const good = { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_A, { message: c.message }), form: 'message', vault: VAULT_1, replaces: 0 };
    const bad: unknown[] = [
      null,
      { ...good, vault: undefined },
      { ...good, vault: '' },
      { ...good, vault: 5 },
      { ...good, vault: { text: 'x' } },
      { ...good, replaces: undefined },
      { ...good, replaces: -1 },
      { ...good, replaces: '0' },
      { ...good, replaces: 0.5 },
      { ...good, nonce: 'zz' },
      { ...good, signature: '0x' },
      { ...good, form: 'other' },
      { ...good, address: 'nope' },
    ];
    for (const b of bad) {
      const r = await handleReplace(env.deps, b);
      expect(r.status).toBe(400);
      expect(r.body).toEqual({ error: 'invalid-request' });
    }
    expect(env.verifier.calls).toHaveLength(0);
    expect(env.store.writes).toHaveLength(0);
  });

  it('answers 503 storage-unavailable when the storage fails, and writes nothing', async () => {
    const env = makeEnv();
    env.store.failing = true;
    const r = await replace(env, VAULT_1, 0);
    expect(r.status).toBe(503);
    expect(r.body).toEqual({ error: 'storage-unavailable' });
    expect(env.store.writes).toHaveLength(0);
  });

  it('keeps accounts apart', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    const b = await replace(env, VAULT_2, 0, { address: ADDR_B });
    expect(b.status).toBe(200);
    expect((await env.store.readCurrent(CHAIN, ADDR_A_LC))!.vault).toBe(VAULT_1);
    expect((await env.store.readCurrent(CHAIN, ADDR_B))!.vault).toBe(VAULT_2);
  });
});

describe('current', () => {
  it('refuses a missing, malformed or wrongly schemed Authorization header', async () => {
    const env = makeEnv();
    const token = body(await replace(env, VAULT_1, 0)).readToken as string;
    for (const authorization of [undefined, '', 'Bearer', 'Bearer ', `Basic ${token}`, token, `Bearer ${token} extra`, 'Bearer junk']) {
      const r = await handleCurrent(env.deps, { address: ADDR_A, chainId: String(CHAIN), authorization });
      expect(r.status).toBe(401);
      expect(r.body).toEqual({ error: 'token-invalid' });
    }
    expect((await handleCurrent(env.deps, { address: ADDR_A, chainId: String(CHAIN), authorization: `bearer ${token}` })).status).toBe(200);
  });

  it("refuses a token for another account, and never returns that account's vault", async () => {
    const env = makeEnv();
    const tokenA = body(await replace(env, VAULT_1, 0)).readToken as string;
    await replace(env, VAULT_2, 0, { address: ADDR_B });
    const r = await current(env, tokenA, ADDR_B);
    expect(r.status).toBe(401);
    expect(r.body).toEqual({ error: 'token-invalid' });
  });

  it('refuses an expired token', async () => {
    const env = makeEnv();
    const token = body(await replace(env, VAULT_1, 0)).readToken as string;
    env.clock.t += 30 * 24 * 3600 * 1000 - 1000;
    expect((await current(env, token)).status).toBe(200);
    env.clock.t += 1000;
    const r = await current(env, token);
    expect(r.status).toBe(401);
    expect(r.body).toEqual({ error: 'token-invalid' });
  });

  it('refuses a token made under another secret', async () => {
    const env = makeEnv();
    const other = makeEnv({ config: { tokenSecret: Buffer.alloc(32, 1) } });
    await replace(env, VAULT_1, 0);
    const foreign = body(await replace(other, VAULT_1, 0)).readToken as string;
    expect((await current(env, foreign)).status).toBe(401);
  });

  it('answers 404 no-vault for a valid token when the record has been removed', async () => {
    const env = makeEnv();
    const token = body(await replace(env, VAULT_1, 0)).readToken as string;
    env.store.remove(CHAIN, ADDR_A_LC);
    const r = await current(env, token);
    expect(r.status).toBe(404);
    expect(r.body).toEqual({ error: 'no-vault' });
  });

  it('refuses a token from before a record was removed and created again', async () => {
    const env = makeEnv();
    const token = body(await replace(env, VAULT_1, 0)).readToken as string;
    env.store.remove(CHAIN, ADDR_A_LC);
    env.clock.t += 60_000;
    expect((await replace(env, VAULT_2, 0)).status).toBe(200);
    const r = await current(env, token);
    expect(r.status).toBe(401);
    expect(r.body).toEqual({ error: 'token-invalid' });
  });

  it('refuses malformed queries with 400', async () => {
    const env = makeEnv();
    const token = body(await replace(env, VAULT_1, 0)).readToken as string;
    for (const q of [
      { address: undefined, chainId: String(CHAIN) },
      { address: 'nope', chainId: String(CHAIN) },
      { address: ADDR_A, chainId: undefined },
      { address: ADDR_A, chainId: 'abc' },
      { address: ADDR_A, chainId: '84532.0' },
      { address: ADDR_A, chainId: '' },
    ]) {
      const r = await handleCurrent(env.deps, { ...q, authorization: `Bearer ${token}` });
      expect(r.status).toBe(400);
      expect(r.body).toEqual({ error: 'invalid-request' });
    }
  });

  it('answers 503 storage-unavailable when the storage fails', async () => {
    const env = makeEnv();
    const token = body(await replace(env, VAULT_1, 0)).readToken as string;
    env.store.failing = true;
    const r = await current(env, token);
    expect(r.status).toBe(503);
    expect(r.body).toEqual({ error: 'storage-unavailable' });
  });

  it('does not touch storage for a bad token', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    env.store.failing = true; // any storage access would turn the answer into 503
    expect((await current(env, 'junk.junk')).status).toBe(401);
  });
});

describe('health', () => {
  it('carries exactly the documented fields and no account data', async () => {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    const r = await handleHealth(env.deps);
    expect(r.status).toBe(200);
    expect(r.body).toEqual({ status: 'ok', region: 'uk', portal: 'uk.portal.example', chainId: CHAIN, storage: 'ok' });
    const text = JSON.stringify(r);
    expect(text).not.toContain(ADDR_A_LC.slice(2));
    expect(text).not.toContain(VAULT_1);
  });

  it('reports a storage error', async () => {
    const env = makeEnv();
    env.store.failing = true;
    const r = await handleHealth(env.deps);
    expect(r.status).toBe(200);
    expect(body(r).storage).toBe('error');
  });
});

describe('logs (R9)', () => {
  it('carry address, purpose, outcome and timing, and never a vault, a signature, a nonce or a token', async () => {
    const env = makeEnv();
    const consoleSpies = (['log', 'info', 'warn', 'error', 'debug'] as const).map((m) => vi.spyOn(console, m).mockImplementation(() => {}));
    const secrets: string[] = [VAULT_1, VAULT_2, sha256Hex(VAULT_1), sha256Hex(VAULT_2)];

    // A full life: create, open in both forms, read, replace, plus every failure path.
    const c0 = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(VAULT_1), replaces: 0 });
    const s0 = fakeSign(ADDR_A, { message: c0.message });
    secrets.push(c0.nonce, s0, s0.slice(2));
    const badSig = '0x' + 'ee'.repeat(65);
    secrets.push(badSig.slice(2));
    await handleReplace(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c0.nonce, signature: badSig, form: 'message', vault: VAULT_1, replaces: 0 });
    env.verifier.mode = 'unavailable';
    await handleReplace(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c0.nonce, signature: s0, form: 'message', vault: VAULT_1, replaces: 0 });
    env.verifier.mode = 'strict';
    const created = await handleReplace(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c0.nonce, signature: s0, form: 'message', vault: VAULT_1, replaces: 0 });
    expect(created.status).toBe(200);
    const token1 = body(created).readToken as string;
    secrets.push(token1, ...token1.split('.'));

    const c1 = await challengeFor(env, 'read');
    const s1 = fakeSign(ADDR_A, { typedData: c1.typedData });
    secrets.push(c1.nonce, s1.slice(2));
    const opened = await handleOpen(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c1.nonce, signature: s1, form: 'typedData' });
    expect(opened.status).toBe(200);
    const token2 = body(opened).readToken as string;
    secrets.push(token2, ...token2.split('.'));
    await handleOpen(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c1.nonce, signature: s1, form: 'typedData' }); // replay
    await current(env, token2);
    await current(env, 'junk.junk');
    await replace(env, VAULT_2, 0); // conflict
    await replace(env, VAULT_2, 1);
    await current(env, token2); // revoked
    await handleOpen(env.deps, { address: 'nope', chainId: CHAIN, nonce: c1.nonce, signature: s1, form: 'message' });
    await handleReplace(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: 'bad', signature: s1, form: 'message', vault: VAULT_2, replaces: 9 });
    env.store.failing = true;
    await open(env);
    await handleHealth(env.deps);

    expect(env.logs.length).toBeGreaterThan(12);
    const text = JSON.stringify(env.logs);
    for (const secret of secrets) {
      expect(secret.length).toBeGreaterThan(8);
      expect(text.includes(secret)).toBe(false);
    }
    for (const spy of consoleSpies) expect(spy).not.toHaveBeenCalled();

    // Only the documented fields, and the useful ones are there.
    const allowed = new Set(['call', 'address', 'purpose', 'outcome', 'status', 'ms', 'kind', 'client']);
    for (const entry of env.logs) {
      for (const key of Object.keys(entry)) expect(allowed.has(key)).toBe(true);
      expect(typeof entry.outcome).toBe('string');
      expect(typeof entry.ms).toBe('number');
    }
    const ok = env.logs.find((e) => e.call === 'replace' && e.outcome === 'ok')!;
    expect(ok).toMatchObject({ address: ADDR_A_LC, purpose: 'replace', status: 200 });
    expect(env.logs.some((e) => e.call === 'open' && e.outcome === 'signature-check-unavailable')).toBe(false);
    expect(env.logs.some((e) => e.call === 'replace' && e.outcome === 'signature-check-unavailable' && e.status === 503)).toBe(true);
    expect(env.logs.some((e) => e.call === 'replace' && e.outcome === 'signature-invalid' && e.address === ADDR_A_LC)).toBe(true);
    expect(env.logs.some((e) => e.call === 'replace' && e.outcome === 'version-conflict')).toBe(true);
    expect(env.logs.some((e) => e.call === 'current' && e.outcome === 'token-invalid')).toBe(true);
    expect(env.logs.some((e) => e.call === 'open' && e.outcome === 'storage-unavailable')).toBe(true);
    // A malformed address is never copied into a log.
    expect(env.logs.some((e) => e.address === 'nope')).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// Fix round 1 (specification v0.2)
// ---------------------------------------------------------------------------

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const CURRENT_KEY = `v1/${CHAIN}/${ADDR_A_LC}/current.json`;
const PREVIOUS_KEY = `v1/${CHAIN}/${ADDR_A_LC}/previous.json`;
const addrN = (i: number) => '0x' + i.toString(16).padStart(40, '0');

describe('a storage write that outlives its timeout (R6a, F1)', () => {
  it('never lands on top of a newer acknowledged replace: nothing else is accepted for the account until it settles', async () => {
    const env = makeEnv({ config: { storageTimeoutMs: 30 } });
    // The write of A's current.json is slow: it lands 150 ms later, long after the timeout.
    let slowed = false;
    env.store.onWrite = async (key) => {
      if (key === CURRENT_KEY && !slowed) {
        slowed = true;
        await sleep(150);
      }
    };

    const a = await replace(env, 'VAULT-A-' + 'a'.repeat(40), 0);
    expect(a.status).toBe(503);
    expect(a.body).toEqual({ error: 'storage-unavailable' });
    expect(await env.store.readCurrent(CHAIN, ADDR_A_LC)).toBeNull(); // A has not landed yet

    // B arrives while A's write is still in flight. Before the fix B was stored
    // and acknowledged, and A then overwrote it.
    const b = await replace(env, 'VAULT-B-' + 'b'.repeat(40), 0);
    expect(b.status).toBe(503);
    expect(b.body).toEqual({ error: 'storage-unavailable' });
    expect(b.headers?.['Retry-After']).toBeDefined();
    expect(env.store.writes).toHaveLength(0);
    // Reads for the account wait too.
    expect((await open(env)).status).toBe(503);

    await sleep(170); // A's write settles
    expect(env.store.writes).toEqual([CURRENT_KEY]);
    expect((await env.store.readCurrent(CHAIN, ADDR_A_LC))!.vault).toContain('VAULT-A-');

    // The account is usable again, and the record is always the last one acknowledged.
    const stale = await replace(env, 'VAULT-B-' + 'b'.repeat(40), 0);
    expect(stale.status).toBe(409);
    expect(stale.body).toEqual({ error: 'version-conflict', currentVersion: 1 });
    const c = await replace(env, 'VAULT-C-' + 'c'.repeat(40), 1);
    expect(c.status).toBe(200);
    await sleep(50);
    const stored = (await env.store.readCurrent(CHAIN, ADDR_A_LC))!;
    expect(stored.version).toBe(2);
    expect(stored.vault).toContain('VAULT-C-');

    expect(env.logs.filter((e) => e.call === 'replace' && e.status === 503).map((e) => e.kind)).toEqual(['timeout', 'unsettled']);
  });

  it('does not write at all when the timeout passes before the write starts', async () => {
    const env = makeEnv({ config: { storageTimeoutMs: 20 } });
    env.store.onRead = async () => {
      await sleep(60);
    };
    const a = await replace(env, VAULT_1, 0);
    expect(a.status).toBe(503);
    await sleep(120);
    expect(env.store.writes).toHaveLength(0);
    env.store.onRead = undefined;
    expect((await replace(env, VAULT_2, 0)).status).toBe(200);
  });

  it('does not write previous.json either when the caller has already gone (an existing record)', async () => {
    const env = makeEnv({ config: { storageTimeoutMs: 20 } });
    expect((await replace(env, VAULT_1, 0)).status).toBe(200);
    const writes = env.store.writes.length;
    env.store.onRead = async () => {
      await sleep(60);
    };
    expect((await replace(env, VAULT_2, 1)).status).toBe(503);
    await sleep(120);
    expect(env.store.writes).toHaveLength(writes);
    expect(await env.store.readPrevious(CHAIN, ADDR_A_LC)).toBeNull();
    env.store.onRead = undefined;
    expect((await env.store.readCurrent(CHAIN, ADDR_A_LC))!.vault).toBe(VAULT_1);
  });

  it('does not go on to write current.json once its previous.json write completes, when the caller was already answered 503 (round 2)', async () => {
    const env = makeEnv({ config: { storageTimeoutMs: 20 } });
    expect((await replace(env, VAULT_1, 0)).status).toBe(200);
    const writes = env.store.writes.length;
    // previous.json is slow: the timeout passes while it is being written.
    env.store.onWrite = async (key) => {
      if (key === PREVIOUS_KEY) await sleep(70);
    };
    const r = await replace(env, VAULT_2, 1);
    expect(r.status).toBe(503);
    expect(env.logs.at(-1)).toMatchObject({ call: 'replace', kind: 'timeout' });
    await sleep(120); // the previous.json write has completed and the work has settled
    env.store.onWrite = undefined;
    // previous.json landed (it equals current); current.json was NOT written for a caller already told 503.
    expect(env.store.writes.slice(writes)).toEqual([PREVIOUS_KEY]);
    const now = (await env.store.readCurrent(CHAIN, ADDR_A_LC))!;
    expect(now.version).toBe(1);
    expect(now.vault).toBe(VAULT_1);
    expect((await env.store.readPrevious(CHAIN, ADDR_A_LC))!.vault).toBe(VAULT_1);
    // The account is free again and the retry goes through.
    expect((await replace(env, VAULT_2, 1)).status).toBe(200);
  });

  it('another account is not held up', async () => {
    const env = makeEnv({ config: { storageTimeoutMs: 20 } });
    env.store.onWrite = async (key) => {
      if (key === CURRENT_KEY) await sleep(100);
    };
    expect((await replace(env, VAULT_1, 0)).status).toBe(503);
    expect((await replace(env, VAULT_2, 0, { address: ADDR_B })).status).toBe(200);
    await sleep(110);
  });
});

describe('a damaged record: current missing while previous exists (R6c, F3)', () => {
  async function damaged() {
    const env = makeEnv();
    await replace(env, VAULT_1, 0);
    const second = await replace(env, VAULT_2, 1);
    env.store.remove(CHAIN, ADDR_A_LC, 'current');
    expect(await env.store.readPrevious(CHAIN, ADDR_A_LC)).not.toBeNull();
    return { env, token: body(second).readToken as string, writes: env.store.writes.length };
  }

  it('open answers 503 storage-unavailable, never "no vault"', async () => {
    const { env } = await damaged();
    const r = await open(env);
    expect(r.status).toBe(503);
    expect(r.body).toEqual({ error: 'storage-unavailable' });
    expect(env.logs.at(-1)).toMatchObject({ call: 'open', kind: 'corrupt' });
  });

  it('replace answers 503 and writes nothing, whatever "replaces" says', async () => {
    const { env, writes } = await damaged();
    for (const replaces of [0, 1, 2]) {
      const r = await replace(env, 'new-vault-' + 'n'.repeat(40), replaces);
      expect(r.status).toBe(503);
      expect(r.body).toEqual({ error: 'storage-unavailable' });
    }
    expect(env.store.writes).toHaveLength(writes);
    expect(await env.store.readCurrent(CHAIN, ADDR_A_LC)).toBeNull();
    expect((await env.store.readPrevious(CHAIN, ADDR_A_LC))!.vault).toBe(VAULT_1);
  });

  it('current (with a token) answers 503 as well', async () => {
    const { env, token } = await damaged();
    const r = await current(env, token);
    expect(r.status).toBe(503);
    expect(r.body).toEqual({ error: 'storage-unavailable' });
  });

  it('an account with neither object is still plain "no vault"', async () => {
    const env = makeEnv();
    expect((await open(env)).status).toBe(404);
    expect((await replace(env, VAULT_1, 0)).status).toBe(200);
  });
});

describe('signature limits (R1, F5)', () => {
  it('refuses a signature over 8 KiB with 400 before any chain call, and accepts one at the limit as a signature', async () => {
    const env = makeEnv();
    const c = await challengeFor(env, 'read');
    const base = { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, form: 'message' };
    const over = await handleOpen(env.deps, { ...base, signature: '0x' + 'ab'.repeat(8193) });
    expect(over.status).toBe(400);
    expect(over.body).toEqual({ error: 'invalid-request' });
    expect(env.verifier.calls).toHaveLength(0);
    const overReplace = await handleReplace(env.deps, { ...base, signature: '0x' + 'ab'.repeat(8193), vault: VAULT_1, replaces: 0 });
    expect(overReplace.status).toBe(400);
    expect(env.verifier.calls).toHaveLength(0);

    const atLimit = await handleOpen(env.deps, { ...base, signature: '0x' + 'ab'.repeat(8192) });
    expect(atLimit.status).toBe(401);
    expect(atLimit.body).toEqual({ error: 'signature-invalid' });
    expect(env.verifier.calls).toHaveLength(1);
  });

  it('runs at most 16 verifications at once; the next is 503 with Retry-After and keeps its nonce', async () => {
    const env = makeEnv({ wrapVerifier: (v) => withConcurrencyLimit(v, 16) });
    env.verifier.delayMs = 40;
    const requests = [];
    for (let i = 1; i <= 17; i++) {
      const c = await challengeFor(env, 'read', {}, addrN(i));
      requests.push({ address: addrN(i), chainId: CHAIN, nonce: c.nonce, signature: fakeSign(addrN(i), { message: c.message }), form: 'message' });
    }
    const results = await Promise.all(requests.map((q) => handleOpen(env.deps, q)));
    const statuses = results.map((r) => r.status);
    expect(statuses.filter((s) => s === 404)).toHaveLength(16);
    expect(statuses.filter((s) => s === 503)).toHaveLength(1);
    const refusedAt = statuses.indexOf(503);
    expect(results[refusedAt]!.body).toEqual({ error: 'signature-check-unavailable' });
    expect(Number(results[refusedAt]!.headers?.['Retry-After'])).toBeGreaterThan(0);
    expect(env.verifier.calls).toHaveLength(16);
    // The refused caller's nonce is still live: the same request now goes through.
    expect((await handleOpen(env.deps, requests[refusedAt]!)).status).toBe(404);
  });
});

describe('read token bound to region and host (R5, F6)', () => {
  it('refuses a token minted by another region or another host under the same secret', async () => {
    const uk = makeEnv();
    const eu = makeEnv({ config: { region: 'eu' } });
    const otherHost = makeEnv({ config: { portal: 'eu.portal.example' } });
    await replace(uk, VAULT_1, 0);
    for (const foreign of [eu, otherHost]) {
      const token = body(await replace(foreign, VAULT_1, 0)).readToken as string;
      expect((await current(foreign, token)).status).toBe(200);
      const r = await current(uk, token);
      expect(r.status).toBe(401);
      expect(r.body).toEqual({ error: 'token-invalid' });
    }
  });
});

describe('limits per client (R7, F7)', () => {
  it('caps live challenges per client at 20: 429 for that client only, another client is unaffected', async () => {
    const env = makeEnv();
    for (let i = 1; i <= 20; i++) {
      expect((await handleChallenge(env.deps, { address: addrN(i), chainId: CHAIN, purpose: 'read' }, 'client-a')).status).toBe(200);
    }
    const refused = await handleChallenge(env.deps, { address: addrN(21), chainId: CHAIN, purpose: 'read' }, 'client-a');
    expect(refused.status).toBe(429);
    expect(refused.body).toEqual({ error: 'too-many-challenges' });
    expect(refused.headers?.['Retry-After']).toBeDefined();
    expect(env.logs.at(-1)).toMatchObject({ call: 'challenge', status: 429, client: 'client-a' });

    // Client B is not affected, for a new address and for one client A already asked for.
    expect((await handleChallenge(env.deps, { address: addrN(21), chainId: CHAIN, purpose: 'read' }, 'client-b')).status).toBe(200);
    const shared = await handleChallenge(env.deps, { address: addrN(1), chainId: CHAIN, purpose: 'read' }, 'client-b');
    expect(shared.status).toBe(200);
    // And client A can still fetch the challenges it already holds.
    expect((await handleChallenge(env.deps, { address: addrN(1), chainId: CHAIN, purpose: 'read' }, 'client-a')).body.nonce).toBe(shared.body.nonce);
  });

  it('a stranger at their cap cannot stop an owner getting a challenge for the owner\'s own address', async () => {
    const env = makeEnv();
    for (let i = 1; i <= 20; i++) await handleChallenge(env.deps, { address: addrN(i), chainId: CHAIN, purpose: 'read' }, 'stranger');
    // Many strangers hammering the owner's address change nothing for the owner either.
    for (let i = 0; i < 30; i++) await handleChallenge(env.deps, { address: ADDR_A, chainId: CHAIN, purpose: 'read' }, `stranger-${i}`);
    const mine = await handleChallenge(env.deps, { address: ADDR_A, chainId: CHAIN, purpose: 'read' }, 'owner');
    expect(mine.status).toBe(200);
  });

  it('caps signature verifications per client per minute: 429 rate-limited, the chain is not asked, the nonce stays live', async () => {
    const env = makeEnv({ verifyPerMinute: 10 });
    await replace(env, VAULT_1, 0, {}); // one verification by the default client
    const c = await challengeFor(env, 'read');
    const bad = { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: '0x' + 'ee'.repeat(65), form: 'message' };
    for (let i = 0; i < 10; i++) expect((await handleOpen(env.deps, bad, 'client-a')).status).toBe(401);
    const calls = env.verifier.calls.length;
    const refused = await handleOpen(env.deps, bad, 'client-a');
    expect(refused.status).toBe(429);
    expect(refused.body).toEqual({ error: 'rate-limited' });
    expect(Number(refused.headers?.['Retry-After'])).toBeGreaterThan(0);
    expect(env.verifier.calls).toHaveLength(calls);
    expect(env.logs.at(-1)).toMatchObject({ call: 'open', status: 429, client: 'client-a' });
    const refusedReplace = await handleReplace(env.deps, { ...bad, vault: VAULT_2, replaces: 1 }, 'client-a');
    expect([401, 429]).toContain(refusedReplace.status); // a read nonce: refused before or at the limit, never verified
    expect(env.verifier.calls).toHaveLength(calls);

    // The owner, on another client, is not locked out by client A's attempts on the same account.
    const ok = await handleOpen(env.deps, { ...bad, signature: fakeSign(ADDR_A, { message: c.message }) }, 'client-owner');
    expect(ok.status).toBe(200);

    // A minute later client A may verify again.
    env.clock.t += 60_000;
    const c2 = await challengeFor(env, 'read');
    expect((await handleOpen(env.deps, { ...bad, nonce: c2.nonce }, 'client-a')).status).toBe(401);
  });

  it('requests refused before verification do not count as verifications', async () => {
    const env = makeEnv({ verifyPerMinute: 2 });
    await replace(env, VAULT_1, 0, {});
    for (let i = 0; i < 20; i++) {
      const r = await handleOpen(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: 'ab'.repeat(24), signature: '0x' + 'ee'.repeat(65), form: 'message' }, 'client-a');
      expect(r.status).toBe(401);
      expect(r.body).toEqual({ error: 'challenge-unknown' });
    }
    const c = await challengeFor(env, 'read');
    const ok = await handleOpen(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_A, { message: c.message }), form: 'message' }, 'client-a');
    expect(ok.status).toBe(200);
  });
});

describe('storage failures are logged with their kind (R9, F9)', () => {
  it('timeout, corrupt and the S3 error code, never the message; no secret material', async () => {
    const env = makeEnv({ config: { storageTimeoutMs: 20 } });
    const created = await replace(env, VAULT_1, 0);
    const token = body(created).readToken as string;

    // the S3 error code
    env.store.onRead = () => {
      throw Object.assign(new Error('SECRET-S3-MESSAGE with request details'), { code: 'AccessDenied' });
    };
    expect((await open(env)).status).toBe(503);
    expect(env.logs.at(-1)).toMatchObject({ call: 'open', outcome: 'storage-unavailable', kind: 'AccessDenied' });
    expect((await current(env, token)).status).toBe(503);
    expect(env.logs.at(-1)).toMatchObject({ call: 'current', kind: 'AccessDenied' });
    expect((await replace(env, VAULT_2, 1)).status).toBe(503);
    expect(env.logs.at(-1)).toMatchObject({ call: 'replace', kind: 'AccessDenied' });

    // an error with no usable code
    env.store.onRead = () => {
      throw new Error('SECRET-S3-MESSAGE again');
    };
    await open(env);
    expect(env.logs.at(-1)).toMatchObject({ kind: 'error' });

    // corrupt: a stored object that is not a record
    env.store.onRead = undefined;
    env.store.putRaw(CURRENT_KEY, '{"v":1,"vault":"HALF-A-RECORD-SECRET"');
    expect((await open(env)).status).toBe(503);
    expect(env.logs.at(-1)).toMatchObject({ call: 'open', kind: 'corrupt' });

    // timeout
    env.store.onRead = async () => {
      await sleep(60);
    };
    expect((await open(env)).status).toBe(503);
    expect(env.logs.at(-1)).toMatchObject({ call: 'open', kind: 'timeout' });
    await sleep(80);

    const text = JSON.stringify(env.logs);
    for (const secret of ['SECRET-S3-MESSAGE', 'HALF-A-RECORD-SECRET', VAULT_1, VAULT_2, token, ...token.split('.')]) {
      expect(text.includes(secret)).toBe(false);
    }
    // A success carries no kind.
    expect(env.logs.find((e) => e.call === 'replace' && e.outcome === 'ok')!.kind).toBeUndefined();
  });
});

describe('concurrency (F10)', () => {
  it('two replaces in flight for one account with DIFFERENT nonces: exactly one wins, the other gets 409, the record is the winner\'s', async () => {
    const env = makeEnv();
    let release!: () => void;
    const held = new Promise<void>((r) => (release = r));
    let holdFirstRead = true;
    env.store.onRead = async () => {
      if (holdFirstRead) {
        holdFirstRead = false;
        await held;
      }
    };
    const vaultX = 'vault-from-device-X:' + 'x'.repeat(60);
    const vaultY = 'vault-from-device-Y:' + 'y'.repeat(60);

    const cx = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(vaultX), replaces: 0 });
    const px = handleReplace(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: cx.nonce, signature: fakeSign(ADDR_A, { message: cx.message }), form: 'message', vault: vaultX, replaces: 0 });
    await sleep(10); // X has verified, spent its nonce and is waiting on storage
    const cy = await challengeFor(env, 'replace', { vaultSha256: sha256Hex(vaultY), replaces: 0 });
    expect(cy.nonce).not.toBe(cx.nonce);
    const py = handleReplace(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: cy.nonce, signature: fakeSign(ADDR_A, { message: cy.message }), form: 'message', vault: vaultY, replaces: 0 });
    await sleep(10); // Y has verified too: both are in flight
    expect(env.store.writes).toHaveLength(0);
    release();
    const [rx, ry] = await Promise.all([px, py]);

    expect([rx.status, ry.status].sort()).toEqual([200, 409]);
    const winner = rx.status === 200 ? vaultX : vaultY;
    const loser = rx.status === 200 ? ry : rx;
    expect(loser.body).toEqual({ error: 'version-conflict', currentVersion: 1 });
    const stored = (await env.store.readCurrent(CHAIN, ADDR_A_LC))!;
    expect(stored.version).toBe(1);
    expect(stored.vault).toBe(winner);
    expect(env.store.writes).toEqual([CURRENT_KEY]);
  });

  it('an open racing a replace returns the old record or the new one whole, never a mixture', async () => {
    for (const holdOn of ['previous', 'current'] as const) {
      const env = makeEnv();
      const first = await replace(env, VAULT_1, 0);
      const updatedAt1 = body(first).updatedAt as string;
      env.clock.t += 5000;
      let release!: () => void;
      const held = new Promise<void>((r) => (release = r));
      env.store.onWrite = async (key) => {
        if (key === (holdOn === 'previous' ? PREVIOUS_KEY : CURRENT_KEY)) await held;
      };
      const pReplace = replace(env, VAULT_2, 1);
      await sleep(10); // the replace is in the middle of its two writes
      const c = await challengeFor(env, 'read');
      const pOpen = handleOpen(env.deps, { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_A, { message: c.message }), form: 'message' });
      await sleep(10);
      release();
      const [rr, ro] = await Promise.all([pReplace, pOpen]);
      expect(rr.status).toBe(200);
      expect(ro.status).toBe(200);
      const got = body(ro);
      const whole =
        (got.version === 1 && got.vault === VAULT_1 && got.updatedAt === updatedAt1) ||
        (got.version === 2 && got.vault === VAULT_2 && got.updatedAt === body(rr).updatedAt);
      expect(whole, `open during the ${holdOn} write`).toBe(true);
      // The token handed out with the record is good for exactly the record that is now current, or for none.
      const after = await current(env, got.readToken);
      expect(after.status).toBe(got.version === 2 ? 200 : 401);
    }
  });

  it('writeCurrent failing after writePrevious succeeded: 503, and the next open still returns the old vault', async () => {
    const env = makeEnv();
    const first = await replace(env, VAULT_1, 0);
    let failed = 0;
    env.store.onWrite = (key) => {
      if (key === CURRENT_KEY) {
        failed++;
        throw Object.assign(new Error('write refused (test)'), { code: 'SlowDown' });
      }
    };
    const r = await replace(env, VAULT_2, 1);
    expect(r.status).toBe(503);
    expect(r.body).toEqual({ error: 'storage-unavailable' });
    expect(failed).toBe(1);
    expect(env.logs.at(-1)).toMatchObject({ call: 'replace', kind: 'SlowDown' });
    env.store.onWrite = undefined;

    const o = await open(env);
    expect(o.status).toBe(200);
    expect(body(o).version).toBe(1);
    expect(body(o).vault).toBe(VAULT_1);
    // previous.json now equals current.json; the old read token still works; a retry succeeds.
    expect((await env.store.readPrevious(CHAIN, ADDR_A_LC))!.vault).toBe(VAULT_1);
    expect((await current(env, body(first).readToken)).status).toBe(200);
    const retry = await replace(env, VAULT_2, 1);
    expect(retry.status).toBe(200);
    expect(body(retry).version).toBe(2);
  });
});
