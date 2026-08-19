// Per-model pricing: the vault must submit the price the REGISTRY holds for
// (host, model, token), not a service-wide constant.
//
// Why this exists: the constant was 904, the LTX text-to-video rate. Chat
// models on the live chat host are registered at 10000, and the contract
// rejects an underpriced create with "Low price" — so every non-LTX model was
// unopenable. Proven against the live contract by read-only simulation before
// this was written. No chain in CI: the registry read is an injected fake.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { makeVaultChain } from '../src/lib/fiat-vault';
import { ESCROW_INTERFACE } from '../src/lib/escrow';

const VAULT = '0x8ba1f109551bD432803012645Ac136ddd64DBA72';
const MARKETPLACE = '0x00000000000000000000000000000000000000aa';
const USDC = '0x00000000000000000000000000000000000000cc';
const HOST_LTX = '0xabcd000000000000000000000000000000000001';
const HOST_CHAT = '0xabcd000000000000000000000000000000000002';
const MODEL_LTX = `0x${'ab'.repeat(32)}`;
const MODEL_CHAT = `0x${'cd'.repeat(32)}`;

function fakeUsdc() {
  return {
    allowance: async () => 2_000_000n,
    approve: async () => ({ wait: async () => ({}) }),
  };
}

function fakeMarketplace() {
  const calls: unknown[][] = [];
  return {
    calls,
    contract: {
      createSessionJobForModelWithToken: async (...args: unknown[]) => {
        calls.push(args);
        return {
          hash: '0xtx',
          wait: async () => ({
            logs: [
              ESCROW_INTERFACE.encodeEventLog('SessionJobCreatedForModel', [
                7n,
                VAULT,
                args[0] as string,
                args[1] as string,
                500_000n,
              ]),
            ],
          }),
        };
      },
    },
  };
}

/** The registry as the chain holds it today: LTX 904, chat 10000. */
function fakeRegistry(prices: Record<string, bigint>) {
  const reads: Array<{ host: string; modelId: string; token: string }> = [];
  return {
    reads,
    modelPrice: async (host: string, modelId: string, token: string) => {
      reads.push({ host, modelId, token });
      const price = prices[`${host.toLowerCase()}:${modelId.toLowerCase()}`];
      if (price === undefined) throw new Error('model not registered on this host');
      return price;
    },
  };
}

function chainWith(marketplace: ReturnType<typeof fakeMarketplace>, registry: ReturnType<typeof fakeRegistry>) {
  return makeVaultChain({
    vaultAddress: VAULT,
    marketplaceAddress: MARKETPLACE,
    usdcAddress: USDC,
    usdc: fakeUsdc(),
    marketplace: marketplace.contract,
    modelPrice: registry.modelPrice,
  });
}

const PRICES = {
  [`${HOST_LTX.toLowerCase()}:${MODEL_LTX.toLowerCase()}`]: 904n,
  [`${HOST_CHAT.toLowerCase()}:${MODEL_CHAT.toLowerCase()}`]: 10_000n,
};

describe('per-model price resolution', () => {
  beforeEach(() => {
    process.env.FIAT_VAULT_ALLOWANCE_FLOAT_MICRO = '2000000';
    // The old global constant. Nothing below may use it when a registry price exists.
    process.env.NEXT_PUBLIC_SESSION_PRICE_PER_TOKEN = '904';
  });
  afterEach(() => {
    delete process.env.FIAT_VAULT_ALLOWANCE_FLOAT_MICRO;
    delete process.env.NEXT_PUBLIC_SESSION_PRICE_PER_TOKEN;
  });

  it('submits the CHAT model price, not the service constant (the bug this fixes)', async () => {
    const marketplace = fakeMarketplace();
    const registry = fakeRegistry(PRICES);
    await chainWith(marketplace, registry).createSession({
      host: HOST_CHAT,
      modelId: MODEL_CHAT,
      depositMicro: 500_000n,
    });
    // args: host, modelId, token, deposit, PRICE, maxDuration, proofInterval, proofTimeoutWindow
    expect(marketplace.calls[0]![4]).toBe(10_000n);
  });

  it('reads the price per (host, model, token) — the exact triple, not a node default', async () => {
    const registry = fakeRegistry(PRICES);
    await chainWith(fakeMarketplace(), registry).createSession({
      host: HOST_CHAT,
      modelId: MODEL_CHAT,
      depositMicro: 500_000n,
    });
    expect(registry.reads).toEqual([{ host: HOST_CHAT, modelId: MODEL_CHAT, token: USDC }]);
  });

  it('still submits 904 for the LTX model — the video path is unchanged', async () => {
    const marketplace = fakeMarketplace();
    await chainWith(marketplace, fakeRegistry(PRICES)).createSession({
      host: HOST_LTX,
      modelId: MODEL_LTX,
      depositMicro: 500_000n,
    });
    expect(marketplace.calls[0]![4]).toBe(904n);
  });

  it('an explicit pricePerToken still wins and skips the read (caller override)', async () => {
    const marketplace = fakeMarketplace();
    const registry = fakeRegistry(PRICES);
    await chainWith(marketplace, registry).createSession({
      host: HOST_CHAT,
      modelId: MODEL_CHAT,
      depositMicro: 500_000n,
      pricePerToken: 12_345n,
    });
    expect(marketplace.calls[0]![4]).toBe(12_345n);
    expect(registry.reads).toHaveLength(0);
  });

  it('propagates a registry refusal for a model the host does not serve (never guesses)', async () => {
    const marketplace = fakeMarketplace();
    await expect(
      chainWith(marketplace, fakeRegistry(PRICES)).createSession({
        host: HOST_CHAT,
        modelId: MODEL_LTX, // not registered on the chat host
        depositMicro: 500_000n,
      })
    ).rejects.toThrow(/not registered/i);
    expect(marketplace.calls).toHaveLength(0); // nothing submitted, no money moved
  });

  it('refuses a zero price rather than submitting a free session', async () => {
    const marketplace = fakeMarketplace();
    const zero = fakeRegistry({ [`${HOST_CHAT.toLowerCase()}:${MODEL_CHAT.toLowerCase()}`]: 0n });
    await expect(
      chainWith(marketplace, zero).createSession({
        host: HOST_CHAT,
        modelId: MODEL_CHAT,
        depositMicro: 500_000n,
      })
    ).rejects.toThrow(/price/i);
    expect(marketplace.calls).toHaveLength(0);
  });

  it('never falls back to the constant when the registry read fails', async () => {
    const marketplace = fakeMarketplace();
    const broken = {
      reads: [],
      modelPrice: async () => {
        throw new Error('rpc down');
      },
    };
    await expect(
      chainWith(marketplace, broken as unknown as ReturnType<typeof fakeRegistry>).createSession({
        host: HOST_CHAT,
        modelId: MODEL_CHAT,
        depositMicro: 500_000n,
      })
    ).rejects.toThrow(/rpc down|price/i);
    expect(marketplace.calls).toHaveLength(0);
  });
});
