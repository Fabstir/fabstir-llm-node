// OP1.3a: the owner relayer (Add a passkey / Remove), plan D14, T6, T7; check
// findings H2, M1 to M3. Unit tests over a viem custom transport (a scripted
// chain); no network. Each rule here was shown able to fail by a mutation
// (OP1.3a report).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { generatePrivateKey } from 'viem/accounts';
import { decodeFunctionData, parseAbi, type Hex } from 'viem';
import { POST as PREPARE } from '../app/v1/fiat/owners/prepare/route';
import { POST as SUBMIT } from '../app/v1/fiat/owners/submit/route';
import { setFiatDepsForTest } from '../src/lib/fiat-session-service';
import { FiatCredentials } from '../src/lib/fiat-credentials';
import { CreditsLedger, MemoryLedgerStore, type LedgerStore } from '../src/lib/ledger';
import {
  DEFAULT_RELAYER_CONFIG,
  ENTRY_POINT_V06,
  GAS,
  OwnerRelayer,
  accountAddressFor,
  initCodeFor,
  isP256PublicKey,
  ownerChangeCallData,
  relayerConfigFromEnv,
  setOwnerRelayerForTest,
  userOpHash,
  type UserOp,
  type WireUserOp,
} from '../src/lib/owner-relayer';
import { fakeOwnerChain, fakeSign, type FakeChain } from './helpers/fake-owner-chain';
import { softPasskey } from './helpers/soft-passkey';

const T0 = Date.UTC(2026, 10, 23, 10, 0, 0);
const walletAbi = parseAbi([
  'function execute(address target, uint256 value, bytes data) payable',
  'function addOwnerPublicKey(bytes32 x, bytes32 y)',
  'function removeOwnerAtIndex(uint256 index, bytes owner)',
]);
const DAY = 86_400_000;

function fromWire(w: WireUserOp): UserOp {
  return {
    ...w,
    nonce: BigInt(w.nonce),
    callGasLimit: BigInt(w.callGasLimit),
    verificationGasLimit: BigInt(w.verificationGasLimit),
    preVerificationGas: BigInt(w.preVerificationGas),
    maxFeePerGas: BigInt(w.maxFeePerGas),
    maxPriorityFeePerGas: BigInt(w.maxPriorityFeePerGas),
  };
}

type Env = {
  chain: FakeChain;
  relayer: OwnerRelayer;
  privateKey: Hex;
  lines: string[];
  now: { t: number };
  ledger: CreditsLedger;
  credentials: FiatCredentials;
};

async function setup(config: Record<string, unknown> = {}, opts: { store?: LedgerStore } = {}): Promise<Env> {
  const chain = fakeOwnerChain();
  const privateKey = generatePrivateKey();
  const lines: string[] = [];
  const now = { t: T0 };
  const relayer = await OwnerRelayer.open({
    transport: chain.transport,
    chainId: 84532,
    privateKey,
    store: opts.store ?? new MemoryLedgerStore(),
    config,
    now: () => now.t,
    pollingInterval: 5,
    receiptTimeoutMs: 2_000,
    log: (l) => lines.push(l),
    alarm: (l) => lines.push(l),
  });
  const ledger = await CreditsLedger.open(new MemoryLedgerStore());
  const credentials = await FiatCredentials.open(new MemoryLedgerStore());
  setOwnerRelayerForTest(relayer);
  setFiatDepsForTest({ ledger, credentials });
  return { chain, relayer, privateKey, lines, now, ledger, credentials };
}

let purchaseN = 0;
/** A customer: first passkey, address, card credential, and (by default) a credited purchase. */
async function customer(env: Env, opts: { bought?: boolean } = {}) {
  const first = softPasskey();
  const address = accountAddressFor(first.publicKey);
  const token = await env.credentials.issue(address.toLowerCase(), 'browser');
  let pi = '';
  if (opts.bought !== false) {
    purchaseN++;
    pi = `pi_${purchaseN}`;
    await env.ledger.purchase(address.toLowerCase(), 5_000_000n, `evt_${purchaseN}`, { paymentIntentId: pi, grossCents: 600, taxCents: 100 });
  }
  return { first, address, token, pi };
}

const answers: unknown[] = [];
async function call(route: typeof PREPARE, token: string | null, body: unknown): Promise<{ status: number; body: any }> {
  const res = await route(
    new Request('http://fiat/v1/fiat/owners/x', {
      method: 'POST',
      headers: { 'content-type': 'application/json', ...(token ? { authorization: `Bearer ${token}` } : {}) },
      body: typeof body === 'string' ? body : JSON.stringify(body),
    })
  );
  const out = { status: res.status, body: await res.json() };
  answers.push(out);
  return out;
}
const prepare = (token: string | null, body: unknown) => call(PREPARE, token, body);
const submit = (token: string | null, body: unknown) => call(SUBMIT, token, body);

async function addAndSubmit(_env: Env, c: Awaited<ReturnType<typeof customer>>, publicKey: Hex) {
  const p = await prepare(c.token, { action: 'add', publicKey, firstOwnerPublicKey: c.first.publicKey });
  if (p.status !== 200) return p;
  return submit(c.token, { userOpHash: p.body.userOpHash, signature: fakeSign(p.body.userOpHash) });
}

afterEach(() => {
  setOwnerRelayerForTest(undefined);
  setFiatDepsForTest(undefined);
  vi.restoreAllMocks();
});

describe('who may use the relayer (R2)', () => {
  let env: Env;
  beforeEach(async () => {
    env = await setup();
  });

  it('answers 503 relayer_not_configured when the key is not set, before anything else', async () => {
    setOwnerRelayerForTest(null);
    const c = await customer(env);
    expect(await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey })).toEqual({ status: 503, body: { error: 'relayer_not_configured' } });
    expect(relayerConfigFromEnv({}).privateKey).toBeNull();
  });

  it('refuses without a credential and with a wrong one', async () => {
    expect((await prepare(null, { action: 'add', publicKey: softPasskey().publicKey })).body).toEqual({ error: 'credential_required' });
    expect((await prepare('fc1_nope', { action: 'add', publicKey: softPasskey().publicKey })).status).toBe(401);
  });

  it('refuses a fresh account with no credited purchase (sybil), and touches no chain', async () => {
    const c = await customer(env, { bought: false });
    const before = env.chain.state.methods.length;
    const out = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey });
    expect(out).toEqual({ status: 403, body: { error: 'purchase_required' } });
    expect(env.chain.state.methods.length).toBe(before);
    expect((await submit(c.token, { userOpHash: '0x' + '11'.repeat(32), signature: '0x1234' })).body).toEqual({ error: 'purchase_required' });
  });

  it("refuses a first owner key whose address is not the credential's (sender must be the account)", async () => {
    const c = await customer(env);
    const other = softPasskey();
    const out = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: other.publicKey });
    expect(out).toEqual({ status: 403, body: { error: 'wrong_account' } });
  });

  it("the prepared op's sender is the credential's address", async () => {
    const c = await customer(env);
    const out = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey });
    expect(out.status).toBe(200);
    expect(out.body.userOperation.sender).toBe(c.address);
  });
});

describe('the relayer builds the whole op; the client cannot inject (R3, H2)', () => {
  let env: Env;
  beforeEach(async () => {
    env = await setup();
  });

  it('refuses any body field other than action, publicKey, index, firstOwnerPublicKey', async () => {
    const c = await customer(env);
    const base = { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey };
    for (const extra of [
      { callData: '0xdeadbeef' },
      { initCode: '0x' },
      { callGasLimit: '0xffffff' },
      { verificationGasLimit: '0xffffff' },
      { preVerificationGas: '0x1' },
      { maxFeePerGas: '0x1' },
      { sender: c.address },
      { paymasterAndData: '0x12' },
      { nonce: '0x0' },
      { target: c.address },
    ]) {
      expect(await prepare(c.token, { ...base, ...extra }), JSON.stringify(extra)).toEqual({ status: 400, body: { error: 'unexpected_field' } });
    }
    expect((await prepare(c.token, { ...base, action: 'execute' })).body).toEqual({ error: 'bad_action' });
    expect((await prepare(c.token, { ...base, action: 'add', index: 1 })).body).toEqual({ error: 'unexpected_field' });
    expect((await prepare(c.token, { ...base, publicKey: '0x' + '11'.repeat(64) })).body).toEqual({ error: 'bad_public_key' });
    expect((await prepare(c.token, '{"action":')).body).toEqual({ error: 'bad_request' });
  });

  it('the callData is exactly execute(sender, 0, addOwnerPublicKey(x, y)); gas and fees are its own; no paymaster', async () => {
    const c = await customer(env);
    const key = softPasskey().publicKey;
    const out = await prepare(c.token, { action: 'add', publicKey: key, firstOwnerPublicKey: c.first.publicKey });
    const op = fromWire(out.body.userOperation);
    expect(op.callData).toBe(ownerChangeCallData(c.address, { action: 'add', publicKey: key }));
    expect(op.callData.slice(0, 10)).toBe('0xb61d27f6'); // execute
    expect(op.callData).toContain('29565e3b'); // addOwnerPublicKey
    // Decoded independently of the relayer's own encoder: execute(self, 0, addOwnerPublicKey(x, y)).
    const outer = decodeFunctionData({ abi: walletAbi, data: op.callData });
    expect(outer.functionName).toBe('execute');
    expect(outer.args).toEqual([c.address, 0n, expect.any(String)]);
    const inner = decodeFunctionData({ abi: walletAbi, data: outer.args![2] as Hex });
    expect(inner.functionName).toBe('addOwnerPublicKey');
    expect(inner.args).toEqual([`0x${key.slice(2, 66)}`, `0x${key.slice(66)}`]);
    expect(op.callGasLimit).toBe(GAS.callGasLimit);
    expect(op.verificationGasLimit).toBe(GAS.verificationGasLimit);
    expect(op.verificationGasLimit).toBeGreaterThanOrEqual(800_000n);
    expect(op.preVerificationGas).toBe(GAS.preVerificationGas);
    expect(op.maxPriorityFeePerGas).toBe(GAS.maxPriorityFeePerGas);
    expect(op.maxFeePerGas).toBe(env.chain.state.baseFee * 2n + GAS.maxPriorityFeePerGas);
    expect(op.paymasterAndData).toBe('0x');
    expect(op.signature).toBe('0x');
    expect(out.body.userOpHash).toBe(userOpHash(op, 84532));
    expect(out.body.entryPoint).toBe(ENTRY_POINT_V06);
    expect(out.body.chainId).toBe(84532);
  });

  it('a remove is exactly execute(sender, 0, removeOwnerAtIndex(index, owner))', async () => {
    const c = await customer(env);
    const k2 = softPasskey().publicKey;
    env.chain.deploy(c.address, c.first.publicKey);
    env.chain.state.accounts.get(c.address.toLowerCase())!.owners.push(k2.toLowerCase() as Hex);
    const out = await prepare(c.token, { action: 'remove', index: 1, publicKey: k2 });
    expect(out.status).toBe(200);
    const op = fromWire(out.body.userOperation);
    expect(op.callData).toBe(ownerChangeCallData(c.address, { action: 'remove', index: 1, publicKey: k2 }));
    expect(op.callData).toContain('89625b57'); // removeOwnerAtIndex
    const outer = decodeFunctionData({ abi: walletAbi, data: op.callData });
    expect(outer.args).toEqual([c.address, 0n, expect.any(String)]);
    const inner = decodeFunctionData({ abi: walletAbi, data: outer.args![2] as Hex });
    expect(inner).toEqual({ functionName: 'removeOwnerAtIndex', args: [1n, k2.toLowerCase()] });
  });

  it('refuses a fee above its ceiling rather than paying it', async () => {
    const c = await customer(env);
    env.chain.state.baseFee = 2_000_000_000n;
    expect((await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey })).body).toEqual({ error: 'fee_too_high' });
  });

  it('submit takes only the hash and the signature; a signature over other gas fields fails and nothing is sent', async () => {
    const c = await customer(env);
    const out = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey });
    const op = fromWire(out.body.userOperation);
    expect(await submit(c.token, { userOpHash: out.body.userOpHash, signature: fakeSign(out.body.userOpHash), callGasLimit: '0xffffffff' })).toEqual({
      status: 400,
      body: { error: 'unexpected_field' },
    });
    // The client signed a hash over an op with ITS gas (higher limits): the relayer's op does not match it.
    const theirs = userOpHash({ ...op, callGasLimit: 5_000_000n, maxFeePerGas: 10n ** 12n }, 84532);
    const res = await submit(c.token, { userOpHash: out.body.userOpHash, signature: fakeSign(theirs) });
    expect(res).toEqual({ status: 422, body: { error: 'simulation_failed', reason: 'AA24 signature error' } });
    expect(env.chain.state.sent).toEqual([]);
  });
});

describe('initCode and deployment (R3, R4)', () => {
  let env: Env;
  beforeEach(async () => {
    env = await setup();
  });

  it('carries initCode = factory + createAccount([first], 0) only while the account has no code', async () => {
    const c = await customer(env);
    const fresh = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey });
    expect(fresh.body.userOperation.initCode).toBe(initCodeFor(c.first.publicKey));
    env.chain.deploy(c.address, c.first.publicKey);
    const deployed = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey });
    expect(deployed.status).toBe(200);
    expect(deployed.body.userOperation.initCode).toBe('0x');
  });

  it('an undeployed account needs its first owner key', async () => {
    const c = await customer(env);
    expect((await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey })).body).toEqual({ error: 'first_owner_required' });
  });

  it('deployed by someone else between prepare and submit: prepare_again, nothing sent', async () => {
    const c = await customer(env);
    const out = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey });
    env.chain.deploy(c.address, c.first.publicKey);
    expect(await submit(c.token, { userOpHash: out.body.userOpHash, signature: fakeSign(out.body.userOpHash) })).toEqual({ status: 409, body: { error: 'prepare_again' } });
    expect(env.chain.state.sent).toEqual([]);
    // and the stale op is gone
    expect((await submit(c.token, { userOpHash: out.body.userOpHash, signature: fakeSign(out.body.userOpHash) })).status).toBe(404);
  });

  it('a nonce moved since prepare: prepare_again', async () => {
    const c = await customer(env);
    env.chain.deploy(c.address, c.first.publicKey);
    const out = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey });
    env.chain.state.nonces.set(c.address.toLowerCase(), 1n);
    expect((await submit(c.token, { userOpHash: out.body.userOpHash, signature: fakeSign(out.body.userOpHash) })).body).toEqual({ error: 'prepare_again' });
  });
});

describe('submit (R4)', () => {
  let env: Env;
  beforeEach(async () => {
    env = await setup();
  });

  it('adds the passkey: deposits the prefund, sends handleOps, answers the tx hash and owner index 1', async () => {
    const c = await customer(env);
    const k2 = softPasskey().publicKey;
    const res = await addAndSubmit(env, c, k2);
    expect(res.status, JSON.stringify(res.body)).toBe(200);
    expect(res.body.ownerIndex).toBe(1);
    expect(res.body.txHash).toMatch(/^0x[0-9a-f]{64}$/);
    expect(env.chain.state.sent.map((s) => s.functionName)).toEqual(['depositTo', 'handleOps']);
    expect(env.chain.state.sent.every((s) => s.to.toLowerCase() === ENTRY_POINT_V06.toLowerCase())).toBe(true);
    expect(env.chain.state.accounts.get(c.address.toLowerCase())!.owners).toEqual([c.first.publicKey.toLowerCase(), k2.toLowerCase()]);
  });

  it('skips depositTo when the deposit already covers the prefund', async () => {
    const c = await customer(env);
    env.chain.deploy(c.address, c.first.publicKey);
    env.chain.state.deposits.set(c.address.toLowerCase(), 10n ** 16n);
    const p = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey });
    const res = await submit(c.token, { userOpHash: p.body.userOpHash, signature: fakeSign(p.body.userOpHash) });
    expect(res.status).toBe(200);
    expect(env.chain.state.sent.map((s) => s.functionName)).toEqual(['handleOps']);
  });

  it('a wrong signature fails the simulation and sends nothing', async () => {
    const c = await customer(env);
    const p = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey });
    const res = await submit(c.token, { userOpHash: p.body.userOpHash, signature: fakeSign(('0x' + '22'.repeat(32)) as Hex) });
    expect(res.body).toEqual({ error: 'simulation_failed', reason: 'AA24 signature error' });
    expect(env.chain.state.sent).toEqual([]);
  });

  it("an unknown, expired or other account's operation is 404", async () => {
    const c = await customer(env);
    const d = await customer(env);
    expect((await submit(c.token, { userOpHash: '0x' + '11'.repeat(32), signature: '0x1234' })).status).toBe(404);
    const p = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey });
    expect((await submit(d.token, { userOpHash: p.body.userOpHash, signature: fakeSign(p.body.userOpHash) })).status).toBe(404);
    env.now.t += 10 * 60_000 + 1;
    expect((await submit(c.token, { userOpHash: p.body.userOpHash, signature: fakeSign(p.body.userOpHash) })).status).toBe(404);
    expect(env.chain.state.sent).toEqual([]);
  });

  it('one operation per account at a time: a second submit is 409 busy', async () => {
    const c = await customer(env);
    env.chain.deploy(c.address, c.first.publicKey);
    const p1 = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey });
    const p2 = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey });
    let open!: () => void;
    env.chain.state.gate = new Promise<void>((r) => (open = r));
    const first = submit(c.token, { userOpHash: p1.body.userOpHash, signature: fakeSign(p1.body.userOpHash) });
    await vi.waitFor(() => expect(env.chain.state.methods).toContain('eth_sendRawTransaction'));
    const second = await submit(c.token, { userOpHash: p2.body.userOpHash, signature: fakeSign(p2.body.userOpHash) });
    expect(second).toEqual({ status: 409, body: { error: 'busy' } });
    open();
    expect((await first).status).toBe(200);
  });

  it("the account's call failing inside handleOps is operation_failed, with the tx hash", async () => {
    const c = await customer(env);
    env.chain.deploy(c.address, c.first.publicKey);
    const p = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey });
    // The simulation saw it succeed; the chain then disagrees.
    const real = env.relayer.simulate.bind(env.relayer);
    vi.spyOn(env.relayer, 'simulate').mockImplementation(async (op, action) => {
      const r = await real(op, action);
      env.chain.state.failInner = true;
      return r;
    });
    const res = await submit(c.token, { userOpHash: p.body.userOpHash, signature: fakeSign(p.body.userOpHash) });
    expect(res.status).toBe(502);
    expect(res.body.error).toBe('operation_failed');
    expect(res.body.txHash).toMatch(/^0x[0-9a-f]{64}$/);
  });

  it('an already-owner key is refused', async () => {
    const c = await customer(env);
    expect((await prepare(c.token, { action: 'add', publicKey: c.first.publicKey, firstOwnerPublicKey: c.first.publicKey })).body).toEqual({ error: 'already_owner' });
    env.chain.deploy(c.address, c.first.publicKey);
    expect((await prepare(c.token, { action: 'add', publicKey: c.first.publicKey })).body).toEqual({ error: 'already_owner' });
  });

  it('an unfunded gas wallet stops before any send, with an ALARM line', async () => {
    const c = await customer(env);
    env.chain.state.relayerBalance = 1000n;
    const p = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey });
    expect((await submit(c.token, { userOpHash: p.body.userOpHash, signature: fakeSign(p.body.userOpHash) })).body).toEqual({ error: 'relayer_low_balance' });
    expect(env.chain.state.sent).toEqual([]);
    expect(env.lines.some((l) => l.includes('ALARM'))).toBe(true);
  });

  it('logs the low-balance alarm when the wallet is under FIAT_RELAYER_LOW_BALANCE_WEI', async () => {
    env.chain.state.relayerBalance = 10n ** 15n; // under the 0.002 ETH default
    await env.relayer.checkBalance();
    expect(env.lines.filter((l) => l.includes('ALARM') && l.includes('below FIAT_RELAYER_LOW_BALANCE_WEI'))).toHaveLength(1);
  });

  it('a chain that is down is chain_unavailable', async () => {
    const c = await customer(env);
    env.chain.state.down = true;
    expect((await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey })).body).toEqual({ error: 'chain_unavailable' });
  });
});

describe('limits and the removal rules (R5)', () => {
  it('a sixth change for one account in a UTC day is refused; the next day it is allowed again', async () => {
    const env = await setup();
    const c = await customer(env);
    for (let i = 0; i < 5; i++) expect((await addAndSubmit(env, c, softPasskey().publicKey)).status, `add ${i + 1}`).toBe(200);
    expect((await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey })).body).toEqual({ error: 'account_daily_limit' });
    env.now.t += DAY;
    expect((await addAndSubmit(env, c, softPasskey().publicKey)).status).toBe(200);
  });

  it('the per-account cap is checked again at submit', async () => {
    const env = await setup({ maxPerAccountPerDay: 1 });
    const c = await customer(env);
    env.chain.deploy(c.address, c.first.publicKey);
    const p1 = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey });
    const p2 = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey });
    expect((await submit(c.token, { userOpHash: p1.body.userOpHash, signature: fakeSign(p1.body.userOpHash) })).status).toBe(200);
    expect((await submit(c.token, { userOpHash: p2.body.userOpHash, signature: fakeSign(p2.body.userOpHash) })).body).toEqual({ error: 'account_daily_limit' });
  });

  it('the global daily cap is refused across accounts', async () => {
    const env = await setup({ maxPerDay: 2 });
    for (let i = 0; i < 2; i++) {
      const c = await customer(env);
      expect((await addAndSubmit(env, c, softPasskey().publicKey)).status).toBe(200);
    }
    const c = await customer(env);
    expect((await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey })).body).toEqual({ error: 'daily_limit' });
  });

  it('the counts survive a restart (journal)', async () => {
    const store = new MemoryLedgerStore();
    const chain = fakeOwnerChain();
    const open = () =>
      OwnerRelayer.open({ transport: chain.transport, chainId: 84532, privateKey: generatePrivateKey(), store, config: { maxPerAccountPerDay: 1 }, now: () => T0, pollingInterval: 5, log: () => undefined, alarm: () => undefined });
    const r1 = await open();
    const key = softPasskey();
    const userId = accountAddressFor(key.publicKey).toLowerCase();
    const p = await r1.prepare(userId, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: key.publicKey });
    expect((await r1.submit(userId, { userOpHash: p.body.userOpHash, signature: fakeSign(p.body.userOpHash as Hex) })).status).toBe(200);
    const r2 = await open();
    expect((await r2.prepare(userId, { action: 'add', publicKey: softPasskey().publicKey })).body).toEqual({ error: 'account_daily_limit' });
  });

  it('refuses removing the last owner (undeployed: one owner; deployed: one left)', async () => {
    const env = await setup();
    const c = await customer(env);
    expect((await prepare(c.token, { action: 'remove', index: 0, publicKey: c.first.publicKey, firstOwnerPublicKey: c.first.publicKey })).body).toEqual({ error: 'last_owner' });
    // deployed, owners [k1, k2], k1 removed: one left
    const k2 = softPasskey().publicKey;
    env.chain.deploy(c.address, c.first.publicKey);
    const acct = env.chain.state.accounts.get(c.address.toLowerCase())!;
    acct.owners = ['0x', k2.toLowerCase() as Hex];
    acct.removed = 1;
    expect((await prepare(c.token, { action: 'remove', index: 1, publicKey: k2 })).body).toEqual({ error: 'last_owner' });
  });

  it('refuses a remove whose key is not the owner at that index', async () => {
    const env = await setup();
    const c = await customer(env);
    const k2 = softPasskey().publicKey;
    env.chain.deploy(c.address, c.first.publicKey);
    env.chain.state.accounts.get(c.address.toLowerCase())!.owners.push(k2.toLowerCase() as Hex);
    expect((await prepare(c.token, { action: 'remove', index: 1, publicKey: c.first.publicKey })).body).toEqual({ error: 'wrong_owner' });
    expect((await prepare(c.token, { action: 'remove', index: 7, publicKey: k2 })).body).toEqual({ error: 'wrong_owner' });
  });

  it('refuses removing index 0 on the UTC day an add was made; allows it the next day; other indexes are fine', async () => {
    const env = await setup();
    const c = await customer(env);
    const k2 = softPasskey().publicKey;
    expect((await addAndSubmit(env, c, k2)).status).toBe(200);
    expect((await prepare(c.token, { action: 'remove', index: 0, publicKey: c.first.publicKey })).body).toEqual({ error: 'first_passkey_locked_today' });
    expect((await prepare(c.token, { action: 'remove', index: 1, publicKey: k2 })).status).toBe(200);
    env.now.t += DAY;
    const p = await prepare(c.token, { action: 'remove', index: 0, publicKey: c.first.publicKey });
    expect(p.status).toBe(200);
    const res = await submit(c.token, { userOpHash: p.body.userOpHash, signature: fakeSign(p.body.userOpHash) });
    expect(res).toEqual({ status: 200, body: { txHash: expect.stringMatching(/^0x/), ownerIndex: 0, confirmed: true } });
    expect(env.chain.state.accounts.get(c.address.toLowerCase())!.owners[0]).toBe('0x');
  });
});

describe('the key never leaves (T6)', () => {
  it('is in no log line and no answer, across a whole add, refusals and a failure', async () => {
    const logs: string[] = [];
    for (const m of ['log', 'error', 'warn', 'info'] as const) vi.spyOn(console, m).mockImplementation((...a: unknown[]) => void logs.push(a.map(String).join(' ')));
    answers.length = 0;
    const env = await setup();
    const c = await customer(env);
    await addAndSubmit(env, c, softPasskey().publicKey);
    await prepare(c.token, { action: 'remove', index: 0, publicKey: c.first.publicKey });
    await submit(c.token, { userOpHash: '0x' + '11'.repeat(32), signature: '0x12' });
    env.chain.state.down = true;
    await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey });
    env.chain.state.down = false;
    env.chain.state.relayerBalance = 1n;
    await env.relayer.checkBalance();
    const secret = env.privateKey.slice(2).toLowerCase();
    const everything = [...logs, ...env.lines, ...answers.map((a) => JSON.stringify(a))].join('\n').toLowerCase();
    expect(everything.length).toBeGreaterThan(100);
    expect(everything).toContain(env.relayer.address.toLowerCase());
    expect(everything).not.toContain(secret);
    // A malformed key is refused without echoing it.
    const bad = 'ab'.repeat(31);
    relayerConfigFromEnv({ FIAT_RELAYER_PRIVATE_KEY: bad });
    expect(logs.join('\n')).not.toContain(bad);
    expect(relayerConfigFromEnv({ FIAT_RELAYER_PRIVATE_KEY: env.privateKey }).privateKey).toBe(env.privateKey);
  });
});

describe('config and helpers', () => {
  it('reads the caps with their defaults and refuses nonsense', () => {
    const { config } = relayerConfigFromEnv({});
    expect(config.maxPerAccountPerDay).toBe(5);
    expect(config.maxPerDay).toBe(200);
    expect(relayerConfigFromEnv({ FIAT_RELAYER_MAX_PER_ACCOUNT_PER_DAY: '3', FIAT_RELAYER_MAX_PER_DAY: '9', FIAT_RELAYER_LOW_BALANCE_WEI: '7' }).config).toMatchObject({
      maxPerAccountPerDay: 3,
      maxPerDay: 9,
      lowBalanceWei: 7n,
    });
    expect(() => relayerConfigFromEnv({ FIAT_RELAYER_MAX_PER_DAY: '-1' })).toThrow();
    expect(() => relayerConfigFromEnv({ FIAT_RELAYER_LOW_BALANCE_WEI: '1e18' })).toThrow();
  });

  it('checks keys are on P-256', () => {
    expect(isP256PublicKey(softPasskey().publicKey)).toBe(true);
    expect(isP256PublicKey('0x' + '11'.repeat(32) + '22'.repeat(32))).toBe(false);
    expect(isP256PublicKey('0x1234')).toBe(false);
  });

  it("computes the site's known address for the fixture key (same CREATE2 rule as passkey-account.ts)", () => {
    // The plan's G5 fixture (not a curve point: the CREATE2 rule alone).
    expect(accountAddressFor(('0x' + '11'.repeat(32) + '22'.repeat(32)) as Hex)).toBe('0xCf83795Af2E30a8Bb373610f45563eB97A5aEDb1');
  });
});

// ---------------------------------------------------------------------------
// OP1.3a fix pass (temp/own-passkey/OP1.3a-REVIEW.md). Each shown able to fail
// by a mutation (OP1.3a-FIX-REPORT.md).

describe('fix M1: the owner index comes from the AddOwner event in our receipt', () => {
  it('two adds to one account in the same block: our answer is our key’s index, not the last one', async () => {
    const env = await setup();
    const c = await customer(env);
    const ours = softPasskey().publicKey;
    const theirs = softPasskey().publicKey;
    env.chain.state.sameBlockAdd = theirs;
    const res = await addAndSubmit(env, c, ours);
    expect(res).toEqual({ status: 200, body: { txHash: expect.stringMatching(/^0x[0-9a-f]{64}$/), ownerIndex: 1, confirmed: true } });
    // The chain after the block: [first, ours, theirs].
    expect(env.chain.state.accounts.get(c.address.toLowerCase())!.owners).toEqual([c.first.publicKey.toLowerCase(), ours.toLowerCase(), theirs.toLowerCase()]);
  });

  it('no matching AddOwner event: ownerIndex null and the answer says so (never a guessed index), with an ALARM line', async () => {
    const env = await setup();
    const c = await customer(env);
    env.chain.state.dropAddOwnerEvent = true;
    const res = await addAndSubmit(env, c, softPasskey().publicKey);
    expect(res).toEqual({ status: 200, body: { txHash: expect.stringMatching(/^0x/), ownerIndex: null, confirmed: true, note: 'owner_index_unknown' } });
    expect(env.lines.some((l) => l.includes('ALARM') && l.includes('no AddOwner event'))).toBe(true);
  });
});

describe('fix M2: nothing after a mined change turns it into a failure', () => {
  it('the RPC fails and the journal append fails right after the receipt: still 200 with the tx hash, confirmed', async () => {
    const inner = new MemoryLedgerStore();
    const store: LedgerStore = {
      load: () => inner.load(),
      append: async (line: string) => {
        if (line.includes('owner-change-result')) throw new Error('disk full');
        return inner.append(line);
      },
    };
    const env = await setup({}, { store });
    const c = await customer(env);
    env.chain.state.downAfterReceipt = true;
    const res = await addAndSubmit(env, c, softPasskey().publicKey);
    expect(res).toEqual({ status: 200, body: { txHash: expect.stringMatching(/^0x/), ownerIndex: 1, confirmed: true } });
    expect(env.chain.state.down).toBe(true); // the node really was down after the receipt
    expect(env.lines.some((l) => l.includes('result not journaled'))).toBe(true);
  });
});

describe('fix M4: one account cannot fill the prepared table or burn our RPC', () => {
  it('keeps at most 3 open prepared operations per account (a newer one drops the oldest)', async () => {
    const env = await setup();
    const c = await customer(env);
    const hashes: string[] = [];
    for (let i = 0; i < 5; i++) {
      const p = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey });
      expect(p.status).toBe(200);
      hashes.push(p.body.userOpHash);
      env.now.t += 1; // distinct expiry times
    }
    expect(env.relayer.__preparedCount()).toBe(3);
    expect((await submit(c.token, { userOpHash: hashes[0], signature: fakeSign(hashes[0] as Hex) })).status).toBe(404);
    expect((await submit(c.token, { userOpHash: hashes[4], signature: fakeSign(hashes[4] as Hex) })).status).toBe(200);
  });

  it('counts prepare against a daily allowance per account (30), before any chain read; another account is untouched', async () => {
    const env = await setup();
    const c = await customer(env);
    for (let i = 0; i < 30; i++) {
      expect((await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey })).status, `prepare ${i + 1}`).toBe(200);
    }
    const before = env.chain.state.methods.length;
    expect(await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey })).toEqual({ status: 429, body: { error: 'prepare_limit' } });
    expect(env.chain.state.methods.length).toBe(before);
    const d = await customer(env);
    expect((await prepare(d.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: d.first.publicKey })).status).toBe(200);
    env.now.t += DAY;
    expect((await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey })).status).toBe(200);
  });

  it('the global bound is kept', async () => {
    const env = await setup({ maxPrepared: 4 });
    const a = await customer(env);
    const b = await customer(env);
    for (let i = 0; i < 3; i++) expect((await prepare(a.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: a.first.publicKey })).status).toBe(200);
    expect((await prepare(b.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: b.first.publicKey })).status).toBe(200);
    const c = await customer(env);
    expect((await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey })).body).toEqual({ error: 'busy' });
  });
});

describe('fix M5: only a purchase still credited unlocks the relayer', () => {
  it('a purchase refunded in full is refused; a partial refund still counts', async () => {
    const env = await setup();
    const c = await customer(env);
    await env.ledger.chargeRefunded(c.pi, 600, 'evt_full_refund');
    expect(await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey })).toEqual({ status: 403, body: { error: 'purchase_required' } });
    const d = await customer(env);
    await env.ledger.chargeRefunded(d.pi, 300, 'evt_half_refund');
    expect((await prepare(d.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: d.first.publicKey })).status).toBe(200);
  });

  it('a disputed purchase is refused', async () => {
    const env = await setup();
    const c = await customer(env);
    await env.ledger.disputeOpened({ disputeId: 'dp_1', paymentIntentId: c.pi, amountCents: 600, eventId: 'evt_dp_1' });
    expect(await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey })).toEqual({ status: 403, body: { error: 'purchase_required' } });
  });
});

describe('fix L-credential: only the browser credential', () => {
  it("the helper's credential for the same account is 401", async () => {
    const env = await setup();
    const c = await customer(env);
    const helper = await env.credentials.issue(c.address.toLowerCase(), 'helper');
    expect(await prepare(helper, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey })).toEqual({ status: 401, body: { error: 'unauthorised' } });
    expect((await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey })).status).toBe(200);
  });
});

describe('fix L-fee: the fee is checked again at submit', () => {
  it('a base fee that rose past the prepared maxFeePerGas (less the tip) gives prepare_again, nothing sent; a small rise is fine', async () => {
    const env = await setup();
    const c = await customer(env);
    const p = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey });
    expect(BigInt(p.body.userOperation.maxFeePerGas)).toBe(11_000_000n);
    env.chain.state.baseFee = 10_000_001n; // 10,000,001 + 1,000,000 tip > 11,000,000
    expect(await submit(c.token, { userOpHash: p.body.userOpHash, signature: fakeSign(p.body.userOpHash) })).toEqual({ status: 409, body: { error: 'prepare_again' } });
    expect(env.chain.state.sent).toEqual([]);
    const q = await prepare(c.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: c.first.publicKey });
    expect(BigInt(q.body.userOperation.maxFeePerGas)).toBe(21_000_002n); // 2 x 10,000,001 + tip
    env.chain.state.baseFee = 20_000_002n; // base + tip = maxFee exactly: still covered
    expect((await submit(c.token, { userOpHash: q.body.userOpHash, signature: fakeSign(q.body.userOpHash) })).status).toBe(200);
  });
});

describe('fix L-cap: a cap slot is reserved before the send', () => {
  it('two concurrent submits from two accounts with one global place left: one is sent, the other is daily_limit', async () => {
    const env = await setup({ maxPerDay: 1 });
    const a = await customer(env);
    const b = await customer(env);
    const pa = await prepare(a.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: a.first.publicKey });
    const pb = await prepare(b.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: b.first.publicKey });
    const [ra, rb] = await Promise.all([
      submit(a.token, { userOpHash: pa.body.userOpHash, signature: fakeSign(pa.body.userOpHash) }),
      submit(b.token, { userOpHash: pb.body.userOpHash, signature: fakeSign(pb.body.userOpHash) }),
    ]);
    expect([ra.status, rb.status].sort()).toEqual([200, 429]);
    expect([ra.body.error, rb.body.error]).toContain('daily_limit');
    expect(env.chain.state.sent.filter((s) => s.functionName === 'handleOps')).toHaveLength(1);
  });

  it('a refused submit lets its slot go (the cap is not eaten by refusals)', async () => {
    const env = await setup({ maxPerDay: 1 });
    const a = await customer(env);
    const p = await prepare(a.token, { action: 'add', publicKey: softPasskey().publicKey, firstOwnerPublicKey: a.first.publicKey });
    expect((await submit(a.token, { userOpHash: p.body.userOpHash, signature: fakeSign(('0x' + '22'.repeat(32)) as Hex) })).body.error).toBe('simulation_failed');
    expect((await submit(a.token, { userOpHash: p.body.userOpHash, signature: fakeSign(p.body.userOpHash) })).status).toBe(200);
  });
});

describe("fix M3: the relayer's fixed limits are the ones the site checks before signing", () => {
  const SITE = resolve(__dirname, '../../../platformless-blender-website/src/lib/passkey-account.ts');
  it('pins GAS and the fee ceiling', () => {
    expect([GAS.callGasLimit, GAS.verificationGasLimit, GAS.preVerificationGas]).toEqual([200_000n, 800_000n, 60_000n]);
    expect(DEFAULT_RELAYER_CONFIG.maxFeePerGasWei).toBe(1_000_000_000n);
  });
  it.skipIf(!existsSync(SITE))("matches the site's OWNER_CHANGE_LIMITS (when the site tree is next to this one)", () => {
    const block = /export const OWNER_CHANGE_LIMITS = Object\.freeze\(\{([^}]*)\}\)/.exec(readFileSync(SITE, 'utf8'))?.[1] ?? '';
    const num = (name: string) => BigInt((new RegExp(`${name}:\\s*([\\d_]+)n`).exec(block)?.[1] ?? '-1').replace(/_/g, ''));
    expect(num('callGasLimit')).toBe(GAS.callGasLimit);
    expect(num('verificationGasLimit')).toBe(GAS.verificationGasLimit);
    expect(num('preVerificationGas')).toBe(GAS.preVerificationGas);
    expect(num('maxFeePerGas')).toBe(DEFAULT_RELAYER_CONFIG.maxFeePerGasWei);
  });
});
