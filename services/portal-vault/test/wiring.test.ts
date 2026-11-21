// Round 2: the protections that are put together at start-up. Each of them is
// tested on its own elsewhere; these tests build the REAL dependency set the
// server runs with (`buildRuntime`, which `main()` calls) and check that each
// protection is actually wired in and effective, so that none can silently
// disappear from the start-up code while the suite stays green.
import { afterEach, describe, expect, it } from 'vitest';
import { loadConfig } from '../src/config.js';
import { handleOpen, handleReplace, handleChallenge } from '../src/handlers.js';
import {
  MAX_VERIFICATIONS_IN_FLIGHT,
  STORAGE_TIMEOUT_MS,
  VERIFICATIONS_PER_CLIENT_PER_MINUTE,
  buildRuntime,
  type Runtime,
} from '../src/server.js';
import { MemoryVaultStore } from '../src/store.js';
import { ADDR_A, CHAIN, fakeSign, fakeVerifier, sha256Hex } from './helpers.js';

const CONFIG = loadConfig({
  VAULT_REGION: 'uk',
  VAULT_PORTAL_HOST: 'uk.portal.example',
  VAULT_CHAIN_ID: String(CHAIN),
  VAULT_RPC_URLS: 'http://127.0.0.1:1',
  VAULT_TOKEN_SECRET: '00112233445566778899aabbccddeeff'.repeat(2),
  VAULT_S3_ENDPOINT: 'http://127.0.0.1:1',
  VAULT_S3_ACCESS_KEY: 'k',
  VAULT_S3_SECRET_KEY: 's',
  VAULT_S3_BUCKET: 'b',
  VAULT_S3_REGION: 'us-east-1',
});

const runtimes: Runtime[] = [];
function build(over: Parameters<typeof buildRuntime>[1] = {}) {
  const store = new MemoryVaultStore();
  const verifier = fakeVerifier();
  const runtime = buildRuntime(CONFIG, { store, verifier: verifier.verify, log: () => {}, ...over });
  runtimes.push(runtime);
  return { runtime, store, verifier, deps: runtime.deps };
}
afterEach(() => {
  for (const r of runtimes.splice(0)) r.timers.forEach((t) => clearInterval(t));
});

const addrN = (i: number) => '0x' + i.toString(16).padStart(40, '0');
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe('the dependency set the server runs with (round 2)', () => {
  it('uses the documented limits', () => {
    expect(MAX_VERIFICATIONS_IN_FLIGHT).toBe(16);
    expect(VERIFICATIONS_PER_CLIENT_PER_MINUTE).toBe(10);
    expect(STORAGE_TIMEOUT_MS).toBe(10_000);
    const { deps } = build();
    expect(deps.config.storageTimeoutMs).toBe(10_000);
    expect(deps.config.region).toBe('uk');
    expect(deps.config.portal).toBe('uk.portal.example');
    expect(deps.config.chainId).toBe(CHAIN);
    expect(deps.config.readTokenSeconds).toBe(30 * 24 * 3600);
    expect(deps.config.maxVaultBytes).toBe(32768);
  });

  it('refuses the 17th concurrent verification with 503 and Retry-After', async () => {
    const { deps, verifier } = build();
    verifier.delayMs = 60;
    const requests = [];
    for (let i = 1; i <= 17; i++) {
      const c = (await handleChallenge(deps, { address: addrN(i), chainId: CHAIN, purpose: 'read' }, `client-${i}`)).body as any;
      requests.push({ address: addrN(i), chainId: CHAIN, nonce: c.nonce, signature: fakeSign(addrN(i), { message: c.message }), form: 'message' });
    }
    // Seventeen different clients, so only the limit on checks in flight can refuse.
    const results = await Promise.all(requests.map((q, i) => handleOpen(deps, q, `client-${i + 1}`)));
    const statuses = results.map((r) => r.status);
    expect(statuses.filter((s) => s === 404)).toHaveLength(16);
    expect(statuses.filter((s) => s === 503)).toHaveLength(1);
    const refused = results[statuses.indexOf(503)]!;
    expect(refused.body).toEqual({ error: 'signature-check-unavailable' });
    expect(Number(refused.headers?.['Retry-After'])).toBeGreaterThan(0);
    expect(verifier.calls).toHaveLength(16);
  });

  it('refuses the 11th verification from one client in a minute with 429', async () => {
    const { deps, verifier } = build();
    const c = (await handleChallenge(deps, { address: ADDR_A, chainId: CHAIN, purpose: 'read' }, 'client-a')).body as any;
    const attempt = { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: '0x' + 'ee'.repeat(65), form: 'message' };
    for (let i = 0; i < 10; i++) expect((await handleOpen(deps, attempt, 'client-a')).status).toBe(401);
    const eleventh = await handleOpen(deps, attempt, 'client-a');
    expect(eleventh.status).toBe(429);
    expect(eleventh.body).toEqual({ error: 'rate-limited' });
    expect(Number(eleventh.headers?.['Retry-After'])).toBeGreaterThan(0);
    expect(verifier.calls).toHaveLength(10);
    expect((await handleOpen(deps, attempt, 'client-b')).status).toBe(401);
  });

  it('starts the three sweep timers, none of which keeps the process alive, and they sweep', async () => {
    const clock = { t: 1_800_000_000_000 };
    const { runtime, deps } = build({ now: () => clock.t, sweepIntervalMs: 5 });
    expect(runtime.timers).toHaveLength(3);
    for (const t of runtime.timers) expect(t.hasRef()).toBe(false);

    await handleChallenge(deps, { address: ADDR_A, chainId: CHAIN, purpose: 'read' }, 'client-a');
    runtime.rateLimiter.check('client-a');
    deps.verifyLimiter.check('client-a');
    expect([deps.challenges.size(), runtime.rateLimiter.size(), deps.verifyLimiter.size()]).toEqual([1, 1, 1]);
    clock.t += 10 * 60_000; // everything has expired; nothing is calling
    await sleep(40);
    expect([deps.challenges.size(), runtime.rateLimiter.size(), deps.verifyLimiter.size()]).toEqual([0, 0, 0]);
  });

  it('answers 503 (kind timeout) when the storage is slower than the configured timeout', async () => {
    const logs: any[] = [];
    const { deps, store } = build({ storageTimeoutMs: 30, log: (e) => logs.push(e) });
    store.onRead = async () => {
      await sleep(120);
    };
    const vault = 'wiring-test-vault:' + 'w'.repeat(40);
    const c = (await handleChallenge(deps, { address: ADDR_A, chainId: CHAIN, purpose: 'replace', vaultSha256: sha256Hex(vault), replaces: 0 }, 'client-a')).body as any;
    const started = Date.now();
    const r = await handleReplace(deps, { address: ADDR_A, chainId: CHAIN, nonce: c.nonce, signature: fakeSign(ADDR_A, { message: c.message }), form: 'message', vault, replaces: 0 }, 'client-a');
    expect(r.status).toBe(503);
    expect(r.body).toEqual({ error: 'storage-unavailable' });
    expect(Date.now() - started).toBeLessThan(110);
    expect(logs.at(-1)).toMatchObject({ call: 'replace', kind: 'timeout' });
    await sleep(140);
    expect(store.writes).toHaveLength(0);
  });

  it('builds the real S3 store and the real on-chain verifier when nothing is injected', async () => {
    const runtime = buildRuntime(CONFIG, { log: () => {} });
    runtimes.push(runtime);
    expect(runtime.deps.store.constructor.name).toBe('S3VaultStore');
    // The real verifier, behind the limit: a closed loopback port is "unavailable", never "invalid".
    await expect(runtime.deps.verify({ address: ADDR_A, message: 'm', signature: '0x' + 'ab'.repeat(65) })).rejects.toMatchObject({
      name: 'SignatureCheckUnavailableError',
    });
  });
});
