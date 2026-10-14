// Per-model pricing: the vault must submit the price the REGISTRY holds for
// (host, model, token), not a service-wide constant.
//
// Why this exists: the constant was 904, the LTX text-to-video rate. Chat
// models on the live chat host are registered at 10000, and the contract
// rejects an underpriced create with "Low price" — so every non-LTX model was
// unopenable. Proven against the live contract by read-only simulation before
// this was written. No chain in CI: the registry read is an injected fake.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { scriptedVaultChain } from './helpers/vault-harness';

// The scripted chain's USDC (the token the registry is asked about).
const USDC = '0x00000000000000000000000000000000000000cc';
const HOST_LTX = '0xabcd000000000000000000000000000000000001';
const HOST_CHAT = '0xabcd000000000000000000000000000000000002';
const MODEL_LTX = `0x${'ab'.repeat(32)}`;
const MODEL_CHAT = `0x${'cd'.repeat(32)}`;

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

// D20: the vault chain sends only through the outbox; the scripted chain
// records every create it was asked to broadcast (decoded).
async function chainWith(registry: ReturnType<typeof fakeRegistry>) {
  const v = await scriptedVaultChain({ modelPrice: registry.modelPrice });
  return { ...v, createSession: v.vaultChain.createSession.bind(v.vaultChain) };
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
    const registry = fakeRegistry(PRICES);
    const marketplace = await chainWith(registry);
    await marketplace.createSession({
      host: HOST_CHAT,
      modelId: MODEL_CHAT,
      depositMicro: 500_000n,
    });
    // args: host, modelId, token, deposit, PRICE, maxDuration, proofInterval, proofTimeoutWindow
    expect(marketplace.creates()[0]![4]).toBe(10_000n);
  });

  it('reads the price per (host, model, token) — the exact triple, not a node default', async () => {
    const registry = fakeRegistry(PRICES);
    await (await chainWith(registry)).createSession({
      host: HOST_CHAT,
      modelId: MODEL_CHAT,
      depositMicro: 500_000n,
    });
    expect(registry.reads).toEqual([{ host: HOST_CHAT, modelId: MODEL_CHAT, token: USDC }]);
  });

  it('still submits 904 for the LTX model — the video path is unchanged', async () => {
    const marketplace = await chainWith(fakeRegistry(PRICES));
    await marketplace.createSession({
      host: HOST_LTX,
      modelId: MODEL_LTX,
      depositMicro: 500_000n,
    });
    expect(marketplace.creates()[0]![4]).toBe(904n);
  });

  it('an explicit pricePerToken still wins and skips the read (caller override)', async () => {
    const registry = fakeRegistry(PRICES);
    const marketplace = await chainWith(registry);
    await marketplace.createSession({
      host: HOST_CHAT,
      modelId: MODEL_CHAT,
      depositMicro: 500_000n,
      pricePerToken: 12_345n,
    });
    expect(marketplace.creates()[0]![4]).toBe(12_345n);
    expect(registry.reads).toHaveLength(0);
  });

  it('propagates a registry refusal for a model the host does not serve (never guesses)', async () => {
    const marketplace = await chainWith(fakeRegistry(PRICES));
    await expect(
      marketplace.createSession({
        host: HOST_CHAT,
        modelId: MODEL_LTX, // not registered on the chat host
        depositMicro: 500_000n,
      })
    ).rejects.toThrow(/not registered/i);
    expect(marketplace.creates()).toHaveLength(0); // nothing submitted, no money moved
  });

  it('refuses a zero price rather than submitting a free session', async () => {
    const zero = fakeRegistry({ [`${HOST_CHAT.toLowerCase()}:${MODEL_CHAT.toLowerCase()}`]: 0n });
    const marketplace = await chainWith(zero);
    await expect(
      marketplace.createSession({
        host: HOST_CHAT,
        modelId: MODEL_CHAT,
        depositMicro: 500_000n,
      })
    ).rejects.toThrow(/price/i);
    expect(marketplace.creates()).toHaveLength(0);
  });

  it('never falls back to the constant when the registry read fails', async () => {
    const broken = {
      reads: [],
      modelPrice: async () => {
        throw new Error('rpc down');
      },
    };
    const marketplace = await chainWith(broken as unknown as ReturnType<typeof fakeRegistry>);
    await expect(
      marketplace.createSession({
        host: HOST_CHAT,
        modelId: MODEL_CHAT,
        depositMicro: 500_000n,
      })
    ).rejects.toThrow(/rpc down|price/i);
    expect(marketplace.creates()).toHaveLength(0);
  });
});
