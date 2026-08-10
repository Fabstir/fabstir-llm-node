// FC1.2 — the vault signer (Decision 1/3b) and the backend auth signature
// (Decision 7). No chain in CI: the chain surface is injected fakes; the
// signature scheme is locked with a literal vector so the node's Rust verifier
// (FC1.6) has a fixed target.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { Wallet, recoverAddress } from 'ethers';
import {
  SESSION_AUTH_SCHEME,
  backendAuthAddress,
  makeVaultChain,
  sessionAuthDigest,
  signSessionAuthorisation,
} from '../src/lib/fiat-vault';
import { ESCROW_INTERFACE } from '../src/lib/escrow';

const AUTH_KEY = `0x${'11'.repeat(32)}`;
const VAULT = '0x8ba1f109551bD432803012645Ac136ddd64DBA72';
const MARKETPLACE = '0x00000000000000000000000000000000000000aa';
const HOST = '0xabcd000000000000000000000000000000000001';
const MODEL = `0x${'ab'.repeat(32)}`;
const USDC = '0x00000000000000000000000000000000000000cc';

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

type ApproveCall = { spender: string; amount: bigint };

function fakeUsdc(initialAllowance: bigint) {
  const approvals: ApproveCall[] = [];
  let allowance = initialAllowance;
  return {
    approvals,
    setAllowance: (a: bigint) => {
      allowance = a;
    },
    contract: {
      allowance: async () => allowance,
      approve: async (spender: string, amount: bigint) => {
        approvals.push({ spender, amount });
        allowance = amount;
        return { wait: async () => ({}) };
      },
    },
  };
}

function sessionCreatedLog(jobId: bigint, depositor: string, host: string, deposit: bigint) {
  return ESCROW_INTERFACE.encodeEventLog('SessionJobCreatedForModel', [jobId, depositor, host, MODEL, deposit]);
}

function fakeMarketplace(jobId: bigint, depositor: string) {
  const calls: unknown[][] = [];
  return {
    calls,
    contract: {
      createSessionJobForModelWithToken: async (...args: unknown[]) => {
        calls.push(args);
        return {
          hash: '0xtx',
          wait: async () => ({ logs: [sessionCreatedLog(jobId, depositor, args[0] as string, 500_000n)] }),
        };
      },
    },
  };
}

describe('makeVaultChain (fake contracts, no RPC)', () => {
  beforeEach(() => {
    process.env.FIAT_VAULT_ALLOWANCE_FLOAT_MICRO = '2000000';
  });
  afterEach(() => {
    delete process.env.FIAT_VAULT_ALLOWANCE_FLOAT_MICRO;
  });

  function chainWith(usdc: ReturnType<typeof fakeUsdc>, marketplace: ReturnType<typeof fakeMarketplace>) {
    return makeVaultChain({
      vaultAddress: VAULT,
      marketplaceAddress: MARKETPLACE,
      usdcAddress: USDC,
      usdc: usdc.contract,
      marketplace: marketplace.contract,
    });
  }

  it('skips approve when the allowance already covers the deposit', async () => {
    const usdc = fakeUsdc(600_000n);
    const chain = chainWith(usdc, fakeMarketplace(1n, VAULT));
    await chain.ensureAllowance(500_000n);
    expect(usdc.approvals).toHaveLength(0);
  });

  it('tops the allowance up to exactly the configured float, never more', async () => {
    const usdc = fakeUsdc(100_000n);
    const chain = chainWith(usdc, fakeMarketplace(1n, VAULT));
    await chain.ensureAllowance(500_000n);
    expect(usdc.approvals).toEqual([{ spender: MARKETPLACE, amount: 2_000_000n }]);
  });

  it('refuses a deposit above the float (config error, not a bigger approve)', async () => {
    const usdc = fakeUsdc(0n);
    const chain = chainWith(usdc, fakeMarketplace(1n, VAULT));
    await expect(chain.ensureAllowance(2_000_001n)).rejects.toThrow(/float/i);
    expect(usdc.approvals).toHaveLength(0);
  });

  it('creates the session with the proven parameter shape and decodes jobId + depositor', async () => {
    const marketplace = fakeMarketplace(842n, VAULT);
    const chain = chainWith(fakeUsdc(2_000_000n), marketplace);
    const result = await chain.createSession({
      host: HOST,
      modelId: MODEL,
      depositMicro: 500_000n,
      pricePerToken: 904n,
    });
    expect(result).toEqual({ jobId: 842n, depositor: VAULT, txHash: '0xtx' });
    // host, modelId, token, deposit, price, maxDuration, proofInterval, proofTimeoutWindow
    expect(marketplace.calls[0]).toEqual([HOST, MODEL, USDC, 500_000n, 904n, 3600n, 1000n, 300n]);
  });

  it('fails loudly when the receipt carries no SessionJobCreatedForModel event', async () => {
    const marketplace = {
      contract: {
        createSessionJobForModelWithToken: async () => ({ hash: '0xtx', wait: async () => ({ logs: [] }) }),
      },
    };
    const chain = makeVaultChain({
      vaultAddress: VAULT,
      marketplaceAddress: MARKETPLACE,
      usdcAddress: USDC,
      usdc: fakeUsdc(0n).contract,
      marketplace: marketplace.contract,
    });
    await expect(
      chain.createSession({ host: HOST, modelId: MODEL, depositMicro: 500_000n, pricePerToken: 904n })
    ).rejects.toThrow(/SessionJobCreatedForModel/);
  });

  it('rejects a receipt whose depositor is not the vault (mis-wired signer must not pass silently)', async () => {
    const marketplace = fakeMarketplace(842n, HOST); // depositor != vault
    const chain = chainWith(fakeUsdc(2_000_000n), marketplace);
    await expect(
      chain.createSession({ host: HOST, modelId: MODEL, depositMicro: 500_000n, pricePerToken: 904n })
    ).rejects.toThrow(/depositor/);
  });

  it('force-approves and retries ONCE when the create reverts on a stale/insufficient allowance', async () => {
    // The stale-read case: allowance reads as covering the deposit (so
    // ensureAllowance skipped the top-up), but the create reverts "exceeds
    // allowance". createSession must approve the float and retry, then succeed.
    const usdc = fakeUsdc(0n);
    let firstCreate = true;
    const marketplace = {
      contract: {
        createSessionJobForModelWithToken: async (...args: unknown[]) => {
          if (firstCreate) {
            firstCreate = false;
            throw new Error('execution reverted: "ERC20: transfer amount exceeds allowance"');
          }
          return {
            hash: '0xretry',
            wait: async () => ({ logs: [sessionCreatedLog(843n, VAULT, args[0] as string, 500_000n)] }),
          };
        },
      },
    };
    const chain = makeVaultChain({
      vaultAddress: VAULT,
      marketplaceAddress: MARKETPLACE,
      usdcAddress: USDC,
      usdc: usdc.contract,
      marketplace: marketplace.contract,
    });
    const result = await chain.createSession({ host: HOST, modelId: MODEL, depositMicro: 500_000n, pricePerToken: 904n });
    expect(result.jobId).toBe(843n);
    // exactly one recovery approve, to the float
    expect(usdc.approvals).toEqual([{ spender: MARKETPLACE, amount: 2_000_000n }]);
  });

  it('does NOT retry on a non-allowance revert (surfaces the real error)', async () => {
    const usdc = fakeUsdc(2_000_000n);
    const marketplace = {
      contract: {
        createSessionJobForModelWithToken: async () => {
          throw new Error('execution reverted: host not registered');
        },
      },
    };
    const chain = makeVaultChain({
      vaultAddress: VAULT,
      marketplaceAddress: MARKETPLACE,
      usdcAddress: USDC,
      usdc: usdc.contract,
      marketplace: marketplace.contract,
    });
    await expect(
      chain.createSession({ host: HOST, modelId: MODEL, depositMicro: 500_000n, pricePerToken: 904n })
    ).rejects.toThrow(/host not registered/);
    expect(usdc.approvals).toHaveLength(0); // no recovery approve for a non-allowance error
  });
});
