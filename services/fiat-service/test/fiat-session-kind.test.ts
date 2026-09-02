// FT1 — card-paid training sessions. The SERVICE owns the on-chain session
// shape per job kind (a training run lives hours and posts a proof per slice;
// the chat shape fails the node's accept gate AFTER escrow); the caps per kind
// are a separate budget bound to the registered training model ids; every
// policy refusal precedes any chain read and journals no hold; one training
// session is live per user; the reclaimer waits out each kind's own lifetime
// on the contract's proof-silence clock; and a host that has not advertised
// the model is a refusal a client must not retry, not the chain_error it was.
import { afterEach, describe, expect, it } from 'vitest';
import { POST } from '../app/v1/fiat/session/route';
import {
  assertBootInvariants,
  AttemptsBucket,
  getFiatDeps,
  openFiatSession,
  resetFiatBackendForTest,
  setFiatDepsForTest,
  setFiatSessionServiceForTest,
  type FiatSessionDeps,
  type FiatSessionOutcome,
  type FiatSessionRequest,
} from '../src/lib/fiat-session-service';
import { CreditsLedger, LIVE_HOLD_MS, MemoryLedgerStore } from '../src/lib/ledger';
import { FiatCredentials } from '../src/lib/fiat-credentials';
import { IdempotencyStore, requestFingerprint } from '../src/lib/idempotency';
import { makeGatekeeper, type GatekeeperConfig } from '../src/lib/gatekeeper';
import { makeVaultChain, type SessionAuthorisation } from '../src/lib/fiat-vault';
import {
  ESCROW_INTERFACE,
  SESSION_MAX_DURATION,
  SESSION_PROOF_INTERVAL,
  SESSION_PROOF_TIMEOUT_WINDOW,
  TRAINING_SESSION_MAX_DURATION,
  TRAINING_SESSION_PROOF_INTERVAL,
  TRAINING_SESSION_PROOF_TIMEOUT_WINDOW,
  sessionShapeFor,
} from '../src/lib/escrow';
import {
  getProductionSettlementListener,
  msEnv,
  productionMinSpendableMicro,
  productionReclaimDelays,
  RECLAIM_SKEW_MS,
  startProductionSettlementListener,
  startSettlementListener,
  validateReclaimDelays,
} from '../src/lib/settlement-listener';

const HOST = '0xabcd000000000000000000000000000000000001';
const MODEL = `0x${'ab'.repeat(32)}`; // a chat/serving model
const TRAINING_ID = `0x${'a4'.repeat(32)}`; // a registered TRAINING model id
const CLIENT = '0x1234567890abcdef1234567890abcdef12345678';
const VAULT = '0x8ba1f109551bD432803012645Ac136ddd64DBA72';
const USDC = '0x00000000000000000000000000000000000000cc';
const MARKETPLACE = '0x00000000000000000000000000000000000000dd';

const CONFIG: GatekeeperConfig = {
  allowedHosts: [HOST],
  maxDepositPerSessionMicro: 2_000_000n,
  maxDailySpendMicro: 10_000_000n,
  maxOpensPerMinute: 10,
  perKind: { training: { maxDepositPerSessionMicro: 10_000_000n, maxDailySpendMicro: 20_000_000n } },
  trainingModelIds: [TRAINING_ID],
  maxConcurrentTraining: 1,
};
const gate = () => makeGatekeeper(CONFIG);

const fakeSignAuth = (sessionId: bigint, clientAddress: string): SessionAuthorisation => ({
  scheme: 'fc1-session-auth-v1',
  signature: `0xsig-${sessionId}-${clientAddress}`,
  clientAddress,
});

type CreateArgs = Parameters<FiatSessionDeps['chain']['createSession']>[0];

async function makeDeps(
  opts: {
    price?: bigint | Error;
    withPriceReader?: boolean;
    balanceMicro?: bigint;
    attemptsPerMinute?: number;
    config?: GatekeeperConfig;
  } = {}
) {
  const ledgerStore = new MemoryLedgerStore();
  const keyStore = new MemoryLedgerStore();
  const ledger = await CreditsLedger.open(ledgerStore);
  const credentials = await FiatCredentials.open(new MemoryLedgerStore());
  const token = await credentials.issue('user-1');
  await ledger.purchase('user-1', opts.balanceMicro ?? 50_000_000n, 'evt_1');
  const creates: CreateArgs[] = [];
  let reads = 0;
  let nextJob = 900n;
  const chain: FiatSessionDeps['chain'] = {
    ensureAllowance: async () => {},
    createSession: async (args) => {
      creates.push(args);
      return { jobId: nextJob++, depositor: VAULT, txHash: '0xtx' };
    },
    ...(opts.withPriceReader === false
      ? {}
      : {
          modelPrice: async () => {
            reads += 1;
            const p = opts.price ?? 10_000n;
            if (p instanceof Error) throw p;
            return p;
          },
        }),
  };
  const deps: FiatSessionDeps = {
    ledger,
    credentials,
    gatekeeper: makeGatekeeper(opts.config ?? CONFIG),
    chain,
    signAuth: fakeSignAuth,
    idempotency: await IdempotencyStore.open(keyStore),
    ...(opts.attemptsPerMinute !== undefined ? { attempts: new AttemptsBucket(opts.attemptsPerMinute) } : {}),
  };
  // The journals themselves: "no hold journaled" is asserted on the LINES, not
  // on a view a released hold could hide behind (a post-hold read that
  // releases on price 0 leaves every balance view intact and still burns the
  // gross day).
  const holdLines = async () => (await ledgerStore.load()).filter((l) => l.includes('"t":"hold"'));
  const keyLines = async () => keyStore.load();
  return { deps, ledger, token, creates, readCount: () => reads, holdLines, keyLines };
}

const request = (token: string, overrides: Partial<FiatSessionRequest> = {}): FiatSessionRequest => ({
  credential: token,
  host: HOST,
  modelId: MODEL,
  depositMicro: 500_000n,
  clientAddress: CLIENT,
  ...overrides,
});
const training = (token: string, overrides: Partial<FiatSessionRequest> = {}) =>
  request(token, { kind: 'training', modelId: TRAINING_ID, depositMicro: 8_600_000n, ...overrides });

describe('session shape per kind (escrow.ts, D1/D6)', () => {
  afterEach(() => {
    delete process.env.FIAT_SESSION_PROOF_TIMEOUT_WINDOW;
  });

  it('training takes the wallet-path shape the node accepts; standard is unchanged', () => {
    expect(sessionShapeFor('training')).toEqual({
      maxDuration: TRAINING_SESSION_MAX_DURATION,
      proofInterval: TRAINING_SESSION_PROOF_INTERVAL,
      proofTimeoutWindow: TRAINING_SESSION_PROOF_TIMEOUT_WINDOW,
    });
    expect(sessionShapeFor('training')).toEqual({ maxDuration: 14400n, proofInterval: 1000n, proofTimeoutWindow: 3600n });
    expect(sessionShapeFor()).toEqual({
      maxDuration: SESSION_MAX_DURATION,
      proofInterval: SESSION_PROOF_INTERVAL,
      proofTimeoutWindow: SESSION_PROOF_TIMEOUT_WINDOW,
    });
    expect(sessionShapeFor('standard')).toEqual(sessionShapeFor());
  });

  it('the standard proof window is an env knob bounded by the contract maximum', () => {
    process.env.FIAT_SESSION_PROOF_TIMEOUT_WINDOW = '3600';
    expect(sessionShapeFor().proofTimeoutWindow).toBe(3600n);
    expect(sessionShapeFor('training').proofTimeoutWindow).toBe(3600n);
    for (const bad of ['0', '3601', 'soon']) {
      process.env.FIAT_SESSION_PROOF_TIMEOUT_WINDOW = bad;
      expect(() => sessionShapeFor()).toThrow(/FIAT_SESSION_PROOF_TIMEOUT_WINDOW/);
    }
  });
});

describe('the vault create uses the kind shape (D1)', () => {
  function fakeVault() {
    const calls: bigint[][] = [];
    const chain = makeVaultChain({
      vaultAddress: VAULT,
      marketplaceAddress: MARKETPLACE,
      usdcAddress: USDC,
      usdc: { allowance: async () => 100_000_000n, approve: async () => ({ wait: async () => undefined }) },
      marketplace: {
        createSessionJobForModelWithToken: async (_h, _m, _t, deposit, price, maxDuration, proofInterval, proofTimeoutWindow) => {
          calls.push([deposit, price, maxDuration, proofInterval, proofTimeoutWindow]);
          return {
            hash: '0xtx',
            wait: async () => ({
              logs: [
                {
                  topics: [
                    ESCROW_INTERFACE.getEvent('SessionJobCreatedForModel')!.topicHash,
                    '0x' + (901n).toString(16).padStart(64, '0'),
                    '0x' + VAULT.slice(2).toLowerCase().padStart(64, '0'),
                    '0x' + 'ee'.repeat(32),
                  ],
                  data: '0x',
                },
              ],
            }),
          };
        },
      },
      modelPrice: async () => 10_000n,
    });
    return { chain, calls };
  }

  it('passes 14400 / 1000 / 3600 for a training session and 3600 / 1000 / 300 otherwise', async () => {
    const { chain, calls } = fakeVault();
    await chain.createSession({ host: HOST, modelId: TRAINING_ID, depositMicro: 8_600_000n, kind: 'training' });
    await chain.createSession({ host: HOST, modelId: MODEL, depositMicro: 500_000n });
    expect(calls).toEqual([
      [8_600_000n, 10_000n, 14400n, 1000n, 3600n],
      [500_000n, 10_000n, 3600n, 1000n, 300n],
    ]);
  });

  it('exposes the registry price read the service gates on', async () => {
    const { chain } = fakeVault();
    expect(await chain.modelPrice(HOST, MODEL)).toBe(10_000n);
  });
});

describe('openFiatSession with a kind (D1, D2, D10, D11)', () => {
  it('threads kind and the pinned price into the create, and opens the hold under the training caps', async () => {
    const { deps, token, creates, ledger } = await makeDeps();
    // 8.6 USDC is over the 2 USDC standard cap and inside the 10 USDC training cap.
    const outcome = await openFiatSession(deps, training(token));
    expect(outcome.status).toBe('ok');
    expect(creates).toEqual([
      expect.objectContaining({ host: HOST, modelId: TRAINING_ID, depositMicro: 8_600_000n, kind: 'training', pricePerToken: 10_000n }),
    ]);
    expect(ledger.boundJobsWithAge()).toEqual([expect.objectContaining({ jobId: 900n, kind: 'training' })]);
    // The same deposit WITHOUT the kind (on the chat model) is refused by the standard cap, before any chain call.
    const standard = await openFiatSession(deps, request(token, { depositMicro: 8_600_000n }));
    expect(standard).toEqual({ status: 'refused', reason: 'DEPOSIT_OVER_CAP' });
    expect(creates).toHaveLength(1);
  });

  it('a standard open sends NO kind and pins the price it read', async () => {
    const { deps, token, creates } = await makeDeps();
    await openFiatSession(deps, request(token));
    expect(creates[0]).not.toHaveProperty('kind');
    expect(creates[0]!.pricePerToken).toBe(10_000n);
  });

  it('binds the kind to the training model ids in both directions (D10)', async () => {
    const { deps, token, creates, readCount } = await makeDeps();
    // training kind on the chat model
    expect(await openFiatSession(deps, training(token, { modelId: MODEL }))).toEqual({
      status: 'refused',
      reason: 'MODEL_KIND_MISMATCH',
    });
    // the training model id without the kind (a chat-shaped session would fail A.3 after escrow)
    expect(await openFiatSession(deps, request(token, { modelId: TRAINING_ID }))).toEqual({
      status: 'refused',
      reason: 'MODEL_KIND_MISMATCH',
    });
    expect(creates).toEqual([]);
    expect(readCount()).toBe(0);
  });

  it('with no training model ids configured the training kind refuses every open (fail closed)', async () => {
    const { deps, token, creates } = await makeDeps({ config: { ...CONFIG, trainingModelIds: [] } });
    expect(await openFiatSession(deps, training(token))).toEqual({ status: 'refused', reason: 'MODEL_KIND_MISMATCH' });
    expect(creates).toEqual([]);
  });

  it('each kind has its own daily budget (D2)', async () => {
    const { deps, token } = await makeDeps({ balanceMicro: 50_000_000n, config: { ...CONFIG, maxConcurrentTraining: 10 } });
    for (const _ of [1, 2]) {
      const r = await openFiatSession(deps, training(token, { depositMicro: 9_000_000n }));
      expect(r.status).toBe('ok');
    }
    const third = await openFiatSession(deps, training(token, { depositMicro: 9_000_000n }));
    expect(third).toEqual({ status: 'refused', reason: 'DAILY_CAP_EXCEEDED' });
    // 18 USDC of training holds do not touch the 10 USDC standard day.
    const chat = await openFiatSession(deps, request(token, { depositMicro: 2_000_000n }));
    expect(chat.status).toBe('ok');
  });

  it('one training session live per user (D11), free again once it settles', async () => {
    const { deps, token, ledger } = await makeDeps();
    expect((await openFiatSession(deps, training(token))).status).toBe('ok');
    const second = await openFiatSession(deps, training(token, { depositMicro: 1_000_000n }));
    expect(second).toEqual({ status: 'refused', reason: 'CONCURRENT_CAP_EXCEEDED' });
    await ledger.settle(900n, 8_600_000n);
    const after = await openFiatSession(deps, training(token, { depositMicro: 1_000_000n }));
    expect(after.status).toBe('ok');
  });

  it('refuses MODEL_NOT_PRICED with NO hold journaled and the key freed', async () => {
    const { deps, token, creates, ledger, holdLines } = await makeDeps({ price: 0n });
    const outcome = await openFiatSession(deps, training(token, { idempotencyKey: 'k1' }));
    expect(outcome).toEqual({ status: 'refused', reason: 'MODEL_NOT_PRICED' });
    expect(creates).toEqual([]);
    expect(ledger.availableMicro('user-1')).toBe(50_000_000n);
    // The journal has NO hold line (mutation: read after the hold and release
    // on 0 — every view above would still pass, but the gross day would burn).
    expect(await holdLines()).toEqual([]);
    // The key is free again: an honest retry against a now-priced model succeeds.
    deps.chain.modelPrice = async () => 10_000n;
    const retry = await openFiatSession(deps, training(token, { idempotencyKey: 'k1' }));
    expect(retry.status).toBe('ok');
  });

  it('every gatekeeper refusal precedes the price read (D4): off-list host, over-cap deposit, zero balance perform NO read', async () => {
    const off = await makeDeps({ price: 0n });
    expect(await openFiatSession(off.deps, request(off.token, { host: '0x9999999999999999999999999999999999999999' }))).toEqual({
      status: 'refused',
      reason: 'HOST_NOT_ALLOWED',
    });
    expect(off.readCount()).toBe(0);
    const cap = await makeDeps({ price: 0n });
    expect(await openFiatSession(cap.deps, request(cap.token, { depositMicro: 3_000_000n }))).toEqual({
      status: 'refused',
      reason: 'DEPOSIT_OVER_CAP',
    });
    expect(cap.readCount()).toBe(0);
    const broke = await makeDeps({ price: 0n, balanceMicro: 100n });
    expect(await openFiatSession(broke.deps, request(broke.token))).toEqual({
      status: 'refused',
      reason: 'INSUFFICIENT_BALANCE',
    });
    expect(broke.readCount()).toBe(0);
  });

  it('a price READ failure stays chain_error (retryable), with no hold and the key freed', async () => {
    const { deps, token, ledger, holdLines } = await makeDeps({ price: new Error('no backend is currently healthy') });
    const outcome = await openFiatSession(deps, training(token, { idempotencyKey: 'k3' }));
    expect(outcome).toEqual({ status: 'chain_error', message: 'no backend is currently healthy' });
    expect(ledger.availableMicro('user-1')).toBe(50_000_000n);
    expect(await holdLines()).toEqual([]);
    deps.chain.modelPrice = async () => 10_000n;
    expect((await openFiatSession(deps, training(token, { idempotencyKey: 'k3' }))).status).toBe('ok');
  });

  it('a preview refusal frees the key: INSUFFICIENT_BALANCE → purchase → same key succeeds', async () => {
    const { deps, token, ledger } = await makeDeps({ balanceMicro: 100n });
    expect(await openFiatSession(deps, request(token, { idempotencyKey: 'k4' }))).toEqual({
      status: 'refused',
      reason: 'INSUFFICIENT_BALANCE',
    });
    await ledger.purchase('user-1', 1_000_000n, 'evt_2');
    expect((await openFiatSession(deps, request(token, { idempotencyKey: 'k4' }))).status).toBe('ok');
  });

  it('a refusal under the mutex after a passing preview also frees the key', async () => {
    // A gatekeeper that allows the preview and refuses the hold (the preview is advice).
    let calls = 0;
    const { deps, token, ledger } = await makeDeps();
    const real = deps.gatekeeper;
    deps.gatekeeper = (view, req) => {
      calls += 1;
      return calls === 2 ? { allow: false, reason: 'RATE_LIMITED' } : real(view, req);
    };
    expect(await openFiatSession(deps, request(token, { idempotencyKey: 'k5' }))).toEqual({
      status: 'refused',
      reason: 'RATE_LIMITED',
    });
    expect(ledger.unboundHolds()).toEqual([]);
    deps.gatekeeper = real;
    expect((await openFiatSession(deps, request(token, { idempotencyKey: 'k5' }))).status).toBe('ok');
  });

  it('the attempts bucket counts every outcome and a rate-limited attempt journals nothing (D4)', async () => {
    const { deps, token, readCount, keyLines } = await makeDeps({ price: 0n, attemptsPerMinute: 3 });
    for (const _ of [1, 2, 3]) {
      expect(await openFiatSession(deps, training(token))).toEqual({ status: 'refused', reason: 'MODEL_NOT_PRICED' });
    }
    const fourth = await openFiatSession(deps, training(token, { idempotencyKey: 'k6' }));
    expect(fourth).toEqual({ status: 'refused', reason: 'RATE_LIMITED' });
    expect(readCount()).toBe(3);
    // No reserve/release PAIR was written for the rate-limited attempt: the
    // idempotency journal never saw the key (mutation: reserve above the bucket).
    expect((await keyLines()).filter((l) => l.includes('"k6"'))).toEqual([]);
  });

  it('a chain fake without a price reader behaves exactly as before', async () => {
    const { deps, token, creates } = await makeDeps({ withPriceReader: false });
    const outcome = await openFiatSession(deps, request(token));
    expect(outcome.status).toBe('ok');
    expect(creates[0]).not.toHaveProperty('pricePerToken');
  });

  it('the idempotency fingerprint tells a training open from a standard one, and is stable for standard', () => {
    const base = { host: HOST, modelId: MODEL, depositMicro: 500_000n, clientAddress: CLIENT };
    expect(requestFingerprint(base)).toBe(requestFingerprint({ ...base, kind: 'standard' }));
    expect(requestFingerprint(base)).not.toBe(requestFingerprint({ ...base, kind: 'training' }));
  });

  it('replaying a key with a different kind is a key_conflict, not a replay', async () => {
    const { deps, token } = await makeDeps();
    expect((await openFiatSession(deps, training(token, { idempotencyKey: 'k2' }))).status).toBe('ok');
    expect(await openFiatSession(deps, request(token, { idempotencyKey: 'k2' }))).toEqual({ status: 'key_conflict' });
  });
});

describe('the ledger keeps the kind through a journal replay (D3)', () => {
  it('a training hold rebuilt from the journal still counts as training, live, and in the training day', async () => {
    const store = new MemoryLedgerStore();
    const ledger = await CreditsLedger.open(store);
    await ledger.purchase('user-1', 50_000_000n, 'evt_1');
    const open = await ledger.openHold(
      { userId: 'user-1', host: HOST, depositMicro: 9_000_000n, kind: 'training', modelId: TRAINING_ID },
      gate()
    );
    if (!open.ok) throw new Error(open.reason);
    await ledger.bindSession(open.holdId, 4242n);
    const reopened = await CreditsLedger.open(store);
    expect(reopened.boundJobsWithAge()).toEqual([expect.objectContaining({ jobId: 4242n, kind: 'training' })]);
    // Still the user's one live training hold (D11) …
    const second = await reopened.openHold(
      { userId: 'user-1', host: HOST, depositMicro: 1_000_000n, kind: 'training', modelId: TRAINING_ID },
      gate()
    );
    expect(second).toEqual({ ok: false, reason: 'CONCURRENT_CAP_EXCEEDED' });
    // … and it occupies the TRAINING day, not the standard one.
    expect((await reopened.openHold({ userId: 'user-1', host: HOST, depositMicro: 2_000_000n, modelId: MODEL }, gate())).ok).toBe(true);
  });

  it('a stale held hold WITH a tx hash (never bound) no longer counts as live either (D11)', async () => {
    let now = 1_000_000;
    const ledger = await CreditsLedger.open(new MemoryLedgerStore(), { now: () => now });
    await ledger.purchase('user-1', 50_000_000n, 'evt_1');
    const open = await ledger.openHold(
      { userId: 'user-1', host: HOST, depositMicro: 9_000_000n, kind: 'training', modelId: TRAINING_ID },
      gate()
    );
    if (!open.ok) throw new Error(open.reason);
    await ledger.recordCreatePending(open.holdId, '0xevicted');
    now += LIVE_HOLD_MS - 1;
    expect(
      await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: 1_000_000n, kind: 'training', modelId: TRAINING_ID }, gate())
    ).toEqual({ ok: false, reason: 'CONCURRENT_CAP_EXCEEDED' });
    now += 2;
    // Mutation: keep a held hold live whenever txHash is set → this stays refused.
    expect(
      (await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: 1_000_000n, kind: 'training', modelId: TRAINING_ID }, gate())).ok
    ).toBe(true);
  });

  it('a stale held hold (no bind, older than the live window) no longer counts as live (D11)', async () => {
    let now = 1_000_000;
    const ledger = await CreditsLedger.open(new MemoryLedgerStore(), { now: () => now });
    await ledger.purchase('user-1', 50_000_000n, 'evt_1');
    const open = await ledger.openHold(
      { userId: 'user-1', host: HOST, depositMicro: 9_000_000n, kind: 'training', modelId: TRAINING_ID },
      gate()
    );
    expect(open.ok).toBe(true);
    now += LIVE_HOLD_MS - 1;
    expect(
      await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: 1_000_000n, kind: 'training', modelId: TRAINING_ID }, gate())
    ).toEqual({ ok: false, reason: 'CONCURRENT_CAP_EXCEEDED' });
    now += 2;
    expect(
      (await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: 1_000_000n, kind: 'training', modelId: TRAINING_ID }, gate())).ok
    ).toBe(true);
  });
});

describe('the reclaimer (D3)', () => {
  // The knob test below flips the window inline; clear it at describe level too
  // so an assertion failure there cannot leak 3600 into the wrapper tests.
  afterEach(() => delete process.env.FIAT_SESSION_PROOF_TIMEOUT_WINDOW);

  async function ledgerWith(kind: 'standard' | 'training', jobId: bigint) {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    await ledger.purchase('user-1', 50_000_000n, 'evt_1');
    const open = await ledger.openHold(
      {
        userId: 'user-1',
        host: HOST,
        depositMicro: 1_000_000n,
        ...(kind === 'training' ? { kind, modelId: TRAINING_ID } : { modelId: MODEL }),
      },
      gate()
    );
    if (!open.ok) throw new Error(open.reason);
    await ledger.bindSession(open.holdId, jobId);
    return ledger;
  }
  const cursor = { load: async () => 5, save: async () => {} };

  it('a training session older than the standard delay but inside the training delay is left alone', async () => {
    const ledger = await ledgerWith('training', 1201n);
    const reclaimed: bigint[] = [];
    const listener = startSettlementListener({
      ledger,
      source: { latestBlock: async () => 10, query: async () => [] },
      cursor,
      fromBlock: 0,
      onAlarm: () => {},
      reclaim: { trigger: async (jobId) => void reclaimed.push(jobId) },
      reclaimAfterMs: 0,
      reclaimAfterMsByKind: { training: 60 * 60 * 1000 },
      manual: true,
    });
    await listener.tick();
    expect(reclaimed).toEqual([]);
    await listener.stop();
  });

  it('a standard session on the same clock is reclaimed with the UNCHANGED alarm text', async () => {
    const ledger = await ledgerWith('standard', 1202n);
    const reclaimed: bigint[] = [];
    const alarms: string[] = [];
    const listener = startSettlementListener({
      ledger,
      source: { latestBlock: async () => 10, query: async () => [] },
      cursor,
      fromBlock: 0,
      onAlarm: (m) => alarms.push(m),
      reclaim: { trigger: async (jobId) => void reclaimed.push(jobId) },
      reclaimAfterMs: 0,
      reclaimAfterMsByKind: { training: 60 * 60 * 1000 },
      manual: true,
    });
    await listener.tick();
    expect(reclaimed).toEqual([1202n]);
    expect(alarms.some((a) => a.includes('reclaimed stranded session 1202: unsettled'))).toBe(true);
    await listener.stop();
  });

  it('a training session past the training delay IS reclaimed, and the alarm names the kind', async () => {
    const ledger = await ledgerWith('training', 1203n);
    const reclaimed: bigint[] = [];
    const alarms: string[] = [];
    const listener = startSettlementListener({
      ledger,
      source: { latestBlock: async () => 10, query: async () => [] },
      cursor,
      fromBlock: 0,
      onAlarm: (m) => alarms.push(m),
      reclaim: { trigger: async (jobId) => void reclaimed.push(jobId) },
      reclaimAfterMs: 60 * 60 * 1000,
      reclaimAfterMsByKind: { training: 0 },
      manual: true,
    });
    await listener.tick();
    expect(reclaimed).toEqual([1203n]);
    expect(alarms.some((a) => a.includes('reclaimed stranded session 1203 (training): unsettled'))).toBe(true);
    await listener.stop();
  });

  it('the state sweep runs BEFORE the reclaim pass, so a lagging completion settles first', async () => {
    const ledger = await ledgerWith('standard', 1204n);
    const reclaimed: bigint[] = [];
    const listener = startSettlementListener({
      ledger,
      source: { latestBlock: async () => 10, query: async () => [] },
      cursor,
      fromBlock: 0,
      onAlarm: () => {},
      reclaim: { trigger: async (jobId) => void reclaimed.push(jobId) },
      reclaimAfterMs: 0,
      stateSweep: { session: async () => ({ ended: true, refundedToUser: 1_000_000n }) },
      manual: true,
    });
    await listener.tick();
    expect(reclaimed).toEqual([]);
    expect(ledger.refundForJob(1204n)).toBe(1_000_000n);
    await listener.stop();
  });

  it('validates the delays against the contract clock: floors derive from the session shape', () => {
    const standardFloor = (3600 + 300) * 1000 + RECLAIM_SKEW_MS; // 4,500,000
    const trainingFloor = (14400 + 3600) * 1000 + RECLAIM_SKEW_MS; // 18,600,000
    expect(() => validateReclaimDelays({ standard: 2 * 60 * 60 * 1000, byKind: { training: 6 * 60 * 60 * 1000 } })).not.toThrow();
    expect(() => validateReclaimDelays({ standard: standardFloor - 1 })).toThrow(/FIAT_RECLAIM_AFTER_MS/);
    expect(() => validateReclaimDelays({ standard: standardFloor })).not.toThrow();
    expect(() => validateReclaimDelays({ standard: 7_200_000, byKind: { training: trainingFloor - 1 } })).toThrow(
      /FIAT_TRAINING_RECLAIM_AFTER_MS/
    );
    expect(() => validateReclaimDelays({ standard: 7_200_000, byKind: { training: 18_000_000 } })).toThrow(/18600000/);
    for (const bad of [Number.NaN, 0, -1, 1.5]) {
      expect(() => validateReclaimDelays({ standard: bad })).toThrow(/positive integer/);
    }
    // The knob flip moves the standard floor to 7,800,000: a 2 h delay then FAILS.
    process.env.FIAT_SESSION_PROOF_TIMEOUT_WINDOW = '3600';
    expect(() => validateReclaimDelays({ standard: 7_200_000 })).toThrow(/7800000/);
    expect(() => validateReclaimDelays({ standard: 3 * 60 * 60 * 1000 })).not.toThrow();
    delete process.env.FIAT_SESSION_PROOF_TIMEOUT_WINDOW;
  });
});

describe('the production wrapper (D3, D5)', () => {
  const ENV = [
    'FIAT_SETTLEMENT_ENABLED',
    'FIAT_RECLAIM_AFTER_MS',
    'FIAT_TRAINING_RECLAIM_AFTER_MS',
    'FIAT_MIN_SPENDABLE_MICRO',
    'FIAT_VAULT_PRIVATE_KEY',
    'FIAT_TRAINING_MODEL_IDS',
    'FIAT_VAULT_ALLOWANCE_FLOAT_MICRO',
  ];
  afterEach(() => {
    for (const k of ENV) delete process.env[k];
    setFiatDepsForTest(undefined);
    resetFiatBackendForTest();
  });

  it('a blank env value is the default, not zero (the .env.example convention)', () => {
    process.env.FIAT_RECLAIM_AFTER_MS = '';
    expect(msEnv('FIAT_RECLAIM_AFTER_MS', 7_200_000)).toBe(7_200_000);
    process.env.FIAT_RECLAIM_AFTER_MS = '   ';
    expect(msEnv('FIAT_RECLAIM_AFTER_MS', 7_200_000)).toBe(7_200_000);
    process.env.FIAT_TRAINING_RECLAIM_AFTER_MS = '';
    expect(productionReclaimDelays()).toEqual({ standard: 7_200_000, training: 21_600_000 });
    process.env.FIAT_RECLAIM_AFTER_MS = '10800000';
    expect(productionReclaimDelays().standard).toBe(10_800_000);
  });

  it('validates the delays BEFORE the backend is built (a bad delay never reaches the vault key)', async () => {
    process.env.FIAT_SETTLEMENT_ENABLED = '1';
    process.env.FIAT_RECLAIM_AFTER_MS = '3600000'; // below the 4,500,000 floor
    let depsTouched = false;
    setFiatDepsForTest(
      new Proxy({} as FiatSessionDeps, {
        get() {
          depsTouched = true;
          return undefined;
        },
      })
    );
    // The discriminator is `depsTouched`, not the error text: `getFiatDeps()` is
    // async and returns the Proxy, and resolving a promise with it fires the
    // Proxy's `then` probe, so ANY call of getFiatDeps() flips the flag. The
    // flag stays false only because the validator threw before that call
    // (mutation: move the validator after `await getFiatDeps()` → same error
    // message, but depsTouched === true).
    await expect(startProductionSettlementListener()).rejects.toThrow(/FIAT_RECLAIM_AFTER_MS/);
    expect(depsTouched).toBe(false);
    expect(getProductionSettlementListener()).toBeUndefined();
  });

  it('the spendable floor is the largest ENABLED cap, overridable', () => {
    expect(productionMinSpendableMicro({ ...CONFIG, trainingModelIds: [] })).toBe(2_000_000n);
    expect(productionMinSpendableMicro(CONFIG)).toBe(10_000_000n);
    process.env.FIAT_MIN_SPENDABLE_MICRO = '3000000';
    expect(productionMinSpendableMicro(CONFIG)).toBe(3_000_000n);
  });

  it('buildBackend runs the boot invariant before the vault is built', async () => {
    // A training cap above the float, with the kind enabled and NO vault key:
    // the invariant's error, not the "not configured" one, proves the order.
    process.env.FIAT_TRAINING_MODEL_IDS = TRAINING_ID;
    process.env.FIAT_VAULT_ALLOWANCE_FLOAT_MICRO = '2000000';
    resetFiatBackendForTest();
    await expect(getFiatDeps()).rejects.toThrow(/FIAT_VAULT_ALLOWANCE_FLOAT_MICRO.*FIAT_TRAINING_MAX_SESSION_DEPOSIT_MICRO/);
  });
});

describe('boot invariants (D5)', () => {
  afterEach(() => delete process.env.FIAT_SESSION_PROOF_TIMEOUT_WINDOW);

  it('boots with the default float when the training model-id set is empty (training disabled)', () => {
    expect(() => assertBootInvariants({ ...CONFIG, trainingModelIds: [] }, 2_000_000n)).not.toThrow();
  });

  it('refuses to boot when an ENABLED kind\'s cap exceeds the float, naming the offending cap', () => {
    expect(() => assertBootInvariants(CONFIG, 2_000_000n)).toThrow(/FIAT_VAULT_ALLOWANCE_FLOAT_MICRO.*10000000.*FIAT_TRAINING_MAX_SESSION_DEPOSIT_MICRO/);
    expect(() => assertBootInvariants(CONFIG, 10_000_000n)).not.toThrow();
    // The standard cap seeds the maximum, so it can be the offender too.
    expect(() => assertBootInvariants({ ...CONFIG, trainingModelIds: [], maxDepositPerSessionMicro: 3_000_000n }, 2_000_000n)).toThrow(
      /from FIAT_MAX_SESSION_DEPOSIT_MICRO/
    );
  });

  it('refuses to boot on a bad proof-window knob', () => {
    process.env.FIAT_SESSION_PROOF_TIMEOUT_WINDOW = '3601';
    expect(() => assertBootInvariants({ ...CONFIG, trainingModelIds: [] }, 2_000_000n)).toThrow(/FIAT_SESSION_PROOF_TIMEOUT_WINDOW/);
  });
});

describe('POST /v1/fiat/session with kind', () => {
  afterEach(() => setFiatSessionServiceForTest(undefined));

  const okOutcome: FiatSessionOutcome = {
    status: 'ok',
    sessionId: 842n,
    jobId: 842n,
    authorisation: { scheme: 'fc1-session-auth-v1', signature: '0xsig', clientAddress: CLIENT },
  };

  function stub(outcome: FiatSessionOutcome) {
    const seen: FiatSessionRequest[] = [];
    setFiatSessionServiceForTest({
      open: async (req) => {
        seen.push(req);
        return outcome;
      },
    });
    return seen;
  }

  function post(body: unknown) {
    return POST(
      new Request('http://site/v1/fiat/session', {
        method: 'POST',
        headers: { authorization: 'Bearer fc1_token' },
        body: JSON.stringify(body),
      })
    );
  }

  const GOOD = { host: HOST, modelId: TRAINING_ID, depositMicro: '8600000', clientAddress: CLIENT };

  it("passes kind: 'training' through and omits it when absent", async () => {
    const seen = stub(okOutcome);
    expect((await post({ ...GOOD, kind: 'training' })).status).toBe(200);
    expect((await post(GOOD)).status).toBe(200);
    expect(seen[0]).toEqual(expect.objectContaining({ kind: 'training' }));
    expect(seen[1]).not.toHaveProperty('kind');
  });

  it("treats kind: null as absent (the route's idempotencyKey convention; recorded in D1)", async () => {
    const seen = stub(okOutcome);
    expect((await post({ ...GOOD, modelId: MODEL, kind: null })).status).toBe(200);
    expect(seen[0]).not.toHaveProperty('kind');
  });

  it('rejects any other kind (no raw shapes, no other names) before the service is touched', async () => {
    const seen = stub(okOutcome);
    for (const kind of ['video', 'chat', 'standard', 14400, { maxDuration: 14400 }]) {
      const res = await post({ ...GOOD, kind });
      expect(res.status).toBe(400);
      expect(await res.json()).toEqual({ error: "kind must be 'training' when present" });
    }
    expect(seen).toEqual([]);
  });

  it('the new refusals come back as 403 refused with their reason, the way HOST_NOT_ALLOWED does', async () => {
    for (const reason of ['MODEL_NOT_PRICED', 'MODEL_KIND_MISMATCH', 'CONCURRENT_CAP_EXCEEDED'] as const) {
      stub({ status: 'refused', reason });
      const res = await post({ ...GOOD, kind: 'training' });
      expect(res.status).toBe(403);
      expect(await res.json()).toEqual({ error: 'refused', reason });
    }
  });
});
