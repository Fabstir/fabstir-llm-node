// FC1.2 — the vault signer (Decision 1/3b) and the backend auth signature
// (Decision 7). No chain in CI: the chain surface is injected fakes; the
// signature scheme is locked with a literal vector so the node's Rust verifier
// (FC1.6) has a fixed target.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { Wallet, recoverAddress } from 'ethers';
import {
  SESSION_AUTH_SCHEME,
  backendAuthAddress,
  sessionAuthDigest,
  signSessionAuthorisation,
} from '../src/lib/fiat-vault';
import { Transaction } from 'ethers';
import { HOST as SCRIPTED_HOST, MARKETPLACE as SCRIPTED_MARKETPLACE, USDC as SCRIPTED_USDC } from './helpers/scripted-chain';
import { scriptedVaultChain } from './helpers/vault-harness';

const AUTH_KEY = `0x${'11'.repeat(32)}`;
const VAULT = '0x8ba1f109551bD432803012645Ac136ddd64DBA72';
const MODEL = `0x${'ab'.repeat(32)}`;

describe('session authorisation signature (FC1.6 contract)', () => {
  beforeEach(() => {
    process.env.FIAT_BACKEND_AUTH_PRIVATE_KEY = AUTH_KEY;
  });
  afterEach(() => {
    delete process.env.FIAT_BACKEND_AUTH_PRIVATE_KEY;
  });

  it('digest is keccak256("FC1-SESSION-AUTH:<sessionId>:<lowercase client>") — locked vector', () => {
    expect(sessionAuthDigest(818n, '0x1234567890abcDEF1234567890abcdef12345678')).toBe(
      '0x6cba97aea9365ee8f302b9878b72d6b55935bc1e922ed37d9e3da4cdad2f6aee'
    );
  });

  it('signs a 65-byte compact signature recoverable to the auth address, with v in {27,28}', () => {
    const auth = signSessionAuthorisation(818n, '0x1234567890abcdef1234567890abcdef12345678');
    expect(auth.scheme).toBe(SESSION_AUTH_SCHEME);
    expect(auth.signature).toMatch(/^0x[0-9a-f]{130}$/);
    const v = parseInt(auth.signature.slice(-2), 16);
    expect([27, 28]).toContain(v);
    const digest = sessionAuthDigest(818n, auth.clientAddress);
    expect(recoverAddress(digest, auth.signature)).toBe(new Wallet(AUTH_KEY).address);
    expect(backendAuthAddress()).toBe(new Wallet(AUTH_KEY).address);
  });

  it('binds the exact sessionId and client address (different inputs, different digests)', () => {
    const base = sessionAuthDigest(818n, '0x1234567890abcdef1234567890abcdef12345678');
    expect(sessionAuthDigest(819n, '0x1234567890abcdef1234567890abcdef12345678')).not.toBe(base);
    expect(sessionAuthDigest(818n, '0x9999999999999999999999999999999999999999')).not.toBe(base);
  });

  it('throws without the auth key env (never signs with a default)', () => {
    delete process.env.FIAT_BACKEND_AUTH_PRIVATE_KEY;
    expect(() => signSessionAuthorisation(1n, VAULT)).toThrow(/FIAT_BACKEND_AUTH_PRIVATE_KEY/);
  });
});

// D20: makeVaultChain now sends ONLY through the vault outbox, so these run
// over the scripted chain (test/helpers/scripted-chain.ts) instead of raw
// contract fakes. The rules they pin are unchanged: the float, the proven
// create shape, the event decode, the depositor check, the allowance retry.
describe('makeVaultChain (through the outbox, scripted chain, no RPC)', () => {
  beforeEach(() => {
    process.env.FIAT_VAULT_ALLOWANCE_FLOAT_MICRO = '2000000';
  });
  afterEach(() => {
    delete process.env.FIAT_VAULT_ALLOWANCE_FLOAT_MICRO;
  });

  const create = { host: SCRIPTED_HOST, modelId: MODEL, depositMicro: 500_000n, pricePerToken: 904n };

  it('skips approve when the allowance already covers the deposit', async () => {
    const v = await scriptedVaultChain();
    v.chain.allowance = 600_000n;
    await v.vaultChain.createSession(create);
    expect(v.approvals()).toHaveLength(0);
  });

  it('tops the allowance up to exactly the configured float, never more', async () => {
    const v = await scriptedVaultChain();
    v.chain.allowance = 100_000n;
    await v.vaultChain.createSession(create);
    // mutation: approve any other spender (e.g. the USDC token itself) → red
    // here, and the scripted chain raises no allowance for it either.
    expect(v.approvals()).toEqual([2_000_000n]);
    expect(v.chain.broadcastsOf('approve')[0]!.decoded).toEqual({
      kind: 'approve',
      spender: SCRIPTED_MARKETPLACE,
      amount: 2_000_000n,
    });
  });

  it('refuses a deposit above the float (config error, not a bigger approve)', async () => {
    const v = await scriptedVaultChain();
    v.chain.allowance = 0n;
    await expect(v.vaultChain.createSession({ ...create, depositMicro: 2_000_001n })).rejects.toThrow(/float/i);
    expect(v.chain.broadcasts).toHaveLength(0);
  });

  it('creates the session with the proven parameter shape and decodes jobId + depositor', async () => {
    const v = await scriptedVaultChain();
    const result = await v.vaultChain.createSession(create);
    expect(result.jobId).toBe(1001n);
    expect(result.depositor).toBe(v.chain.vaultAddress);
    expect(result.txHash).toBe(v.chain.broadcastsOf('create')[0]!.hash);
    // host, modelId, token, deposit, price, maxDuration, proofInterval, proofTimeoutWindow
    expect(v.creates()[0]).toEqual([SCRIPTED_HOST, MODEL, SCRIPTED_USDC, 500_000n, 904n, 3600n, 1000n, 300n]);
  });

  it('fails loudly when the receipt carries no SessionJobCreatedForModel event', async () => {
    const v = await scriptedVaultChain();
    v.chain.onBroadcast = (raw) => {
      v.chain.mineAll();
      const hash = Transaction.from(raw).hash!;
      v.chain.mined.set(hash, { status: 1, blockNumber: v.chain.block, logs: [] });
    };
    v.chain.autoMine = false;
    await expect(v.vaultChain.createSession(create)).rejects.toThrow(/SessionJobCreatedForModel/);
  });

  it('rejects a receipt whose depositor is not the vault (mis-wired signer must not pass silently)', async () => {
    const v = await scriptedVaultChain();
    v.chain.onBroadcast = (raw) => {
      v.chain.mineAll();
      const hash = Transaction.from(raw).hash!;
      const receipt = v.chain.mined.get(hash)!;
      const log = receipt.logs[0]!;
      const topics = [...log.topics];
      topics[2] = `0x${SCRIPTED_HOST.slice(2).padStart(64, '0')}`; // depositor != vault
      v.chain.mined.set(hash, { ...receipt, logs: [{ topics, data: log.data }] });
    };
    v.chain.autoMine = false;
    await expect(v.vaultChain.createSession(create)).rejects.toThrow(/depositor/);
  });

  it('force-approves and retries ONCE when the create estimate reverts on a stale/insufficient allowance', async () => {
    // The stale-read case: allowance reads as covering the deposit (so the
    // pre-check skipped the top-up), but the create's estimate reverts
    // "insufficient allowance". Approve the float, then populate once more.
    const v = await scriptedVaultChain();
    v.chain.allowance = 0n;
    v.chain.staleAllowanceRead = 2_000_000n;
    const result = await v.vaultChain.createSession(create);
    expect(result.jobId).toBe(1001n);
    expect(v.approvals()).toEqual([2_000_000n]); // exactly one recovery approve, to the float
  });

  it('does NOT retry on a non-allowance revert (surfaces the real error)', async () => {
    const v = await scriptedVaultChain();
    v.chain.onEstimate = (d) => {
      if (d.kind === 'create') throw new Error('execution reverted: host not registered');
    };
    await expect(v.vaultChain.createSession(create)).rejects.toThrow(/host not registered/);
    expect(v.approvals()).toHaveLength(0); // no recovery approve for a non-allowance error
    expect(v.outbox.all()).toHaveLength(0); // the estimate aborts before any append
  });
});
