// D20 items 2–4 (BM1.2c) — R5 over the vault outbox, vault orphans, the
// sweeper through the one outbox, and the pre-deploy (migration) rules. The
// chain is scripted; time is vitest's fake clock. Each test names the
// mutation that turns it red.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IdempotencyStore, requestFingerprint } from '../src/lib/idempotency';
import { MemoryLedgerStore } from '../src/lib/ledger';
import { ESCROW_INTERFACE } from '../src/lib/escrow';
import { longestReclaimKind } from '../src/lib/fiat-reconcile';
import { runProductionVaultReconcile } from '../src/lib/settlement-listener';
import { setFiatDepsForTest } from '../src/lib/fiat-session-service';
import { HOST, MARKETPLACE, MODEL, USDC } from './helpers/scripted-chain';
import { CLIENT, TRAINING_MODEL, USER, makeHarness, type Harness } from './helpers/vault-harness';

const MIN = 60_000;
const HOUR = 60 * MIN;

async function advance<T>(p: Promise<T>, ms: number): Promise<{ done: boolean; value?: T; error?: unknown }> {
  const state: { done: boolean; value?: T; error?: unknown } = { done: false };
  p.then(
    (value) => Object.assign(state, { done: true, value }),
    (error) => Object.assign(state, { done: true, error })
  );
  await vi.advanceTimersByTimeAsync(ms);
  return state;
}

/** A tick that may wait on receipts (sweeps wait up to 90 s). */
async function tick(h: Harness, ms = 100_000): Promise<void> {
  await advance(h.tick(), ms);
}

const fp = requestFingerprint({ host: HOST, modelId: MODEL, depositMicro: 500_000n, clientAddress: CLIENT });

/** A hold opened under `key` and nothing else — the state a crash between
 *  the ledger write and the outbox append leaves. */
async function holdWithoutEntry(h: Harness, key: string) {
  await h.idempotency.reserve(USER, key, fp, CLIENT);
  const open = await h.ledger.openHold({ userId: USER, host: HOST, depositMicro: 500_000n, modelId: MODEL, key }, h.deps.gatekeeper);
  if (!open.ok) throw new Error('refused');
  return open.holdId;
}

/** An open whose create MINED but whose hold was released as `dropped`
 *  (a lagging replica hid the receipt through the whole margin), with the
 *  receipt now visible: the late-receipt case. */
async function releasedButMined(h: Harness, overrides: Parameters<Harness['open']>[0] = {}) {
  const open = h.open({ idempotencyKey: 'k1', ...overrides });
  await vi.advanceTimersByTimeAsync(0);
  const hash = h.chain.broadcastsOf('create')[0]!.hash;
  h.chain.hiddenReceipts.add(hash);
  h.chain.mineAll();
  expect((await advance(open, 171_000)).value).toMatchObject({ status: 'chain_pending' });
  await tick(h, 0); // margin starts
  h.chain.block += 12;
  await vi.advanceTimersByTimeAsync(10 * MIN);
  await tick(h, 0); // dropped → released
  expect(h.ledger.hold('h0')?.state).toBe('released');
  expect(h.ledger.openVaultOrphans()).toHaveLength(0);
  h.chain.hiddenReceipts.delete(hash);
  return { hash, jobId: h.chain.jobIdOf(hash)! };
}

beforeEach(() => {
  vi.useFakeTimers({ now: new Date('2026-10-08T00:00:00Z') });
});
afterEach(() => {
  vi.useRealTimers();
});

describe('R5: keys and holds from the outbox verdicts', () => {
  it('a create mined with status 1 but NO SessionJobCreatedForModel event: the hold stays held, the key pending, ONE alarm for a person', async () => {
    // mutation: release it like a revert → the user is credited back while a
    // session may exist under an event the service cannot read.
    const h = await makeHarness();
    h.chain.onBroadcast = (raw, d) => {
      if (d.kind !== 'create') return;
      h.chain.mineAll();
      const hash = h.chain.broadcastsOf('create')[0]!.hash;
      h.chain.mined.set(hash, { status: 1, blockNumber: h.chain.block, logs: [] });
      void raw;
    };
    const outcome = (await advance(h.open({ idempotencyKey: 'k1' }), 5_000)).value;
    expect(outcome).toMatchObject({ status: 'chain_pending', message: expect.stringMatching(/event inventory mismatch/) });
    expect(h.outbox.all()[0]).toMatchObject({ kind: 'create', outcome: 'mined' });
    expect(h.outbox.all()[0]!.jobId).toBeUndefined();
    await tick(h);
    await vi.advanceTimersByTimeAsync(11 * MIN);
    await tick(h);
    expect(h.ledger.hold('h0')?.state).toBe('held');
    expect((await h.idempotency.lookup(USER, 'k1'))?.state).toBe('pending');
    expect(h.ledger.availableMicro(USER)).toBe(9_500_000n);
    expect(h.alarms.filter((a) => /no SessionJobCreatedForModel event/.test(a))).toHaveLength(1);
  });

  it('a `created` orphan (the open gave up before binding) is bound AND its key completed', async () => {
    // mutation: bind only → the key stays pending for ever.
    const h = await makeHarness();
    const open = h.open({ idempotencyKey: 'k1' });
    await vi.advanceTimersByTimeAsync(0);
    const hash = h.chain.broadcastsOf('create')[0]!.hash;
    h.chain.hiddenReceipts.add(hash);
    h.chain.mineAll();
    expect((await advance(open, 171_000)).value).toMatchObject({ status: 'chain_pending' });
    h.chain.hiddenReceipts.delete(hash);
    await tick(h);
    expect(h.ledger.hold('h0')?.state).toBe('bound');
    const key = await h.idempotency.lookup(USER, 'k1');
    expect(key?.state === 'done' && key.jobId).toBe(h.chain.jobIdOf(hash));
    // A same-key retry now replays the session (the authorisation re-derived).
    expect(await h.open({ idempotencyKey: 'k1' })).toMatchObject({ status: 'ok', replayed: true });
  });

  it('a bound hold whose key is still pending (crash between bind and complete) has its key completed', async () => {
    // mutation: bind only (no key completion) → pending for ever.
    const h = await makeHarness();
    const holdId = await holdWithoutEntry(h, 'k1');
    await h.ledger.bindSession(holdId, 4242n);
    await tick(h);
    const key = await h.idempotency.lookup(USER, 'k1');
    expect(key?.state === 'done' && key.jobId).toBe(4242n);
  });

  it('a pending key follows its hold once it is no longer held: released → freed, bound → completed', async () => {
    // mutation: follow only `held` holds → both keys pending for ever.
    const h = await makeHarness();
    const released = await holdWithoutEntry(h, 'gone');
    await h.ledger.releaseHold(released);
    const bound = await holdWithoutEntry(h, 'kept');
    await h.ledger.bindSession(bound, 4343n);
    await tick(h);
    expect(await h.idempotency.lookup(USER, 'gone')).toBeNull();
    const kept = await h.idempotency.lookup(USER, 'kept');
    expect(kept?.state === 'done' && kept.jobId).toBe(4343n);
  });

  it('a held hold older than 10 min that NO outbox create entry links is released and its key freed', async () => {
    // mutation: release at once → a live-looking hold released before 10 min.
    const h = await makeHarness();
    await holdWithoutEntry(h, 'k1');
    await vi.advanceTimersByTimeAsync(5 * MIN);
    await tick(h);
    expect(h.ledger.hold('h0')?.state).toBe('held');
    expect((await h.idempotency.lookup(USER, 'k1'))?.state).toBe('pending');
    await vi.advanceTimersByTimeAsync(6 * MIN);
    await tick(h);
    expect(h.ledger.hold('h0')?.state).toBe('released');
    expect(await h.idempotency.lookup(USER, 'k1')).toBeNull();
    expect(h.ledger.availableMicro(USER)).toBe(10_000_000n);
  });

  it('a reserved key with no hold is freed only after 10 min, and never while in flight', async () => {
    // mutation: free at once → a live open's key is freed and `complete` lands on nothing.
    const h = await makeHarness();
    await h.idempotency.reserve(USER, 'orphan-key', fp, CLIENT);
    await h.idempotency.reserve(USER, 'live-key', fp, CLIENT);
    h.outbox.inFlight.keys.add(`${USER} live-key`);
    await vi.advanceTimersByTimeAsync(5 * MIN);
    await tick(h);
    expect((await h.idempotency.lookup(USER, 'orphan-key'))?.state).toBe('pending');
    await vi.advanceTimersByTimeAsync(6 * MIN);
    await tick(h);
    expect(await h.idempotency.lookup(USER, 'orphan-key')).toBeNull();
    expect((await h.idempotency.lookup(USER, 'live-key'))?.state).toBe('pending');
  });

  it('in-flight holds and keys are never touched by R5 (a live open whose create mined but has not yet bound)', async () => {
    // mutation: no in-flight guard → R5 binds the hold and completes the key
    // while the request is live.
    const h = await makeHarness();
    h.chain.autoMine = true;
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    let gated = false;
    h.chain.onReceipt = async () => {
      if (!gated) {
        gated = true;
        await gate; // the open's own confirmation read stalls
      }
    };
    const open = h.open({ idempotencyKey: 'k1' });
    await vi.advanceTimersByTimeAsync(0);
    await tick(h, 0); // resolution sees the receipt; R5 must skip the live open
    expect(h.outbox.all()[0]!.outcome).toBe('mined');
    expect(h.ledger.hold('h0')?.state).toBe('held');
    expect((await h.idempotency.lookup(USER, 'k1'))?.state).toBe('pending');
    release();
    expect((await advance(open, 1_000)).value).toMatchObject({ status: 'ok' });
    expect(h.ledger.hold('h0')?.state).toBe('bound');
  });

  it('pending keys survive pruning; done keys keep the 24 h retention', async () => {
    // mutation: prune every row → a pending key vanishes and a retry opens a second session.
    let now = 1_000_000;
    const store = await IdempotencyStore.open(new MemoryLedgerStore(), { now: () => now, retentionMs: 1000 });
    await store.reserve(USER, 'pending', fp, CLIENT);
    await store.reserve(USER, 'done', fp, CLIENT);
    await store.complete(USER, 'done', 9n, CLIENT);
    now += 5000;
    expect((await store.lookup(USER, 'pending'))?.state).toBe('pending');
    expect(await store.lookup(USER, 'done')).toBeNull();
  });
});

describe('vault orphans', () => {
  it('a receipt for a released hold → an alarm and a durable vault-orphan that survives a restart', async () => {
    // mutation: memory only → forgotten at restart, the deposit never reclaimed.
    const h = await makeHarness();
    const { jobId } = await releasedButMined(h);
    await tick(h, 0);
    expect(h.alarms.join('\n')).toMatch(/vault-orphan: late receipt/);
    expect(h.ledger.availableMicro(USER)).toBe(10_000_000n); // the user, already credited back, is untouched
    const h2 = await h.reboot();
    expect(h2.ledger.openVaultOrphans()).toEqual([expect.objectContaining({ holdId: 'h0', jobId, kind: 'standard', amountMicro: 500_000n })]);
  });

  it('a standard orphan is swept after 2 h through the outbox and CLOSED when its session is not Active (never swept twice)', async () => {
    // mutation: no close → a second sweep attempt every tick.
    const h = await makeHarness();
    const { jobId } = await releasedButMined(h);
    await tick(h, 0);
    await vi.advanceTimersByTimeAsync(119 * MIN);
    await tick(h);
    expect(h.chain.broadcastsOf('sweep')).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(2 * MIN);
    const sweeping = h.tick();
    await vi.advanceTimersByTimeAsync(0);
    expect(h.chain.broadcastsOf('sweep').map((b) => (b.decoded as { jobId: bigint }).jobId)).toEqual([jobId]);
    h.chain.mineAll();
    await advance(sweeping, 5_000);
    await tick(h);
    expect(h.ledger.openVaultOrphans()).toHaveLength(0);
    await tick(h);
    await tick(h);
    expect(h.chain.estimates.filter((e) => e.kind === 'sweep')).toHaveLength(1);
  });

  it('open orphans count in the solvency check\'s escrow term (no false breach)', async () => {
    // mutation: omit them → a false "solvency breach" every tick.
    const h = await makeHarness();
    await releasedButMined(h);
    await tick(h, 0);
    // The vault paid 0.5 USDC into escrow nobody is credited for; the user's
    // 10 USDC is back in their balance.
    const h2 = await h.reboot({ solvency: { holdings: async () => 9_500_000n } });
    await tick(h2);
    expect(h2.ledger.escrowedMicro()).toBe(500_000n);
    expect(h2.alarms.join('\n')).not.toMatch(/solvency breach/);
  });

  it('a vault-orphan applied twice by overlapping ticks is counted once', async () => {
    // mutation: append per tick → its deposit doubled in the escrow term.
    const h = await makeHarness();
    await releasedButMined(h);
    await Promise.all([h.reconcile(), h.reconcile()]);
    expect(h.ledger.openVaultOrphans()).toHaveLength(1);
    expect(h.ledger.escrowedMicro()).toBe(500_000n);
    const lines = (await h.stores.ledger.load()).filter((l) => l.includes('"vault-orphan"'));
    expect(lines).toHaveLength(1);
  });

  it('a vault-orphan for a TRAINING hold waits the training reclaim delay', async () => {
    // mutation: the standard 2 h → swept (its estimate reverting on the real
    // contract) while the training session can still be live.
    const h = await makeHarness();
    await releasedButMined(h, { kind: 'training', modelId: TRAINING_MODEL });
    await tick(h, 0);
    expect(h.ledger.openVaultOrphans()[0]?.kind).toBe('training');
    await vi.advanceTimersByTimeAsync(3 * HOUR);
    await tick(h);
    expect(h.chain.broadcastsOf('sweep')).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(3 * HOUR + MIN);
    await tick(h);
    expect(h.chain.broadcastsOf('sweep')).toHaveLength(1);
  });

  it('an outbox create whose link names no hold → an alarm and a vault-orphan with the training delay', async () => {
    // mutation: ignore it → the deposit is never reclaimed.
    const h = await makeHarness();
    h.chain.autoMine = true;
    const data = ESCROW_INTERFACE.encodeFunctionData('createSessionJobForModelWithToken', [
      HOST, MODEL, USDC, 500_000n, 904n, 3600n, 1000n, 300n,
    ]);
    await h.outbox.vaultSend('create', { to: MARKETPLACE, data }, 'h999');
    await tick(h);
    expect(h.alarms.join('\n')).toMatch(/links no hold/);
    expect(h.ledger.openVaultOrphans()).toEqual([
      expect.objectContaining({ holdId: 'h999', kind: 'training', amountMicro: 500_000n }),
    ]);
  });
});

describe('an unlinked-create orphan waits the LONGEST configured reclaim delay (D21)', () => {
  const unlinkedCreate = async (h: Harness) => {
    h.chain.autoMine = true;
    const data = ESCROW_INTERFACE.encodeFunctionData('createSessionJobForModelWithToken', [
      HOST, MODEL, USDC, 500_000n, 904n, 3600n, 1000n, 300n,
    ]);
    await h.outbox.vaultSend('create', { to: MARKETPLACE, data }, 'h998');
  };
  afterEach(() => {
    delete process.env.FIAT_PASSES_RECLAIM_AFTER_MS;
    setFiatDepsForTest(undefined);
  });

  it('the longest kind ranges over standard, training and passes (training on a tie)', () => {
    expect(longestReclaimKind(2 * HOUR, { training: 6 * HOUR, passes: 6 * HOUR })).toBe('training');
    expect(longestReclaimKind(2 * HOUR, { training: 6 * HOUR, passes: 8 * HOUR })).toBe('passes');
    expect(longestReclaimKind(9 * HOUR, { training: 6 * HOUR, passes: 8 * HOUR })).toBe('standard');
    // A kind absent from the map waits the standard delay.
    expect(longestReclaimKind(2 * HOUR, { passes: 8 * HOUR })).toBe('passes');
  });

  it('with passes set longer than training, the orphan is not swept at the training delay', async () => {
    // mutation: record training's delay specifically → swept at 6 h while a
    // 4-hour passes session (+ its window) could still be live.
    const h = await makeHarness({ passesReclaimAfterMs: 8 * HOUR });
    await unlinkedCreate(h);
    await tick(h);
    expect(h.ledger.openVaultOrphans()).toEqual([expect.objectContaining({ holdId: 'h998', kind: 'passes' })]);
    await vi.advanceTimersByTimeAsync(6 * HOUR + MIN);
    await tick(h);
    expect(h.chain.broadcastsOf('sweep')).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(2 * HOUR);
    await tick(h);
    expect(h.chain.broadcastsOf('sweep')).toHaveLength(1);
  });

  it('the cron reconcile uses the production delays too', async () => {
    // mutation: the cron path passes no delays → the orphan is recorded training.
    process.env.FIAT_PASSES_RECLAIM_AFTER_MS = String(8 * HOUR);
    const h = await makeHarness();
    setFiatDepsForTest(h.deps);
    await unlinkedCreate(h);
    await advance(h.outbox.resolve(), 100_000);
    await advance(runProductionVaultReconcile(() => {}), 100_000);
    expect(h.ledger.openVaultOrphans()).toEqual([expect.objectContaining({ holdId: 'h998', kind: 'passes' })]);
  });
});

describe('the sweeper through the one outbox', () => {
  async function boundJob(h: Harness) {
    h.chain.autoMine = true;
    const outcome = await h.open({ idempotencyKey: 'job' });
    if (outcome.status !== 'ok') throw new Error(outcome.status);
    h.chain.autoMine = false;
    await vi.advanceTimersByTimeAsync(2 * HOUR + MIN);
    return outcome.jobId;
  }

  it('a sweep whose gas estimate throws leaves the in-flight set and is retried next tick', async () => {
    // mutation: leave only on a resolution → never swept again.
    const h = await makeHarness();
    const jobId = await boundJob(h);
    let fail = true;
    h.chain.onEstimate = (d) => {
      if (d.kind === 'sweep' && fail) {
        fail = false;
        throw new Error('execution reverted');
      }
    };
    await tick(h);
    expect(h.outbox.inFlight.sweeps.has(jobId.toString())).toBe(false);
    expect(h.alarms.join('\n')).toMatch(/reclaim failed/);
    await tick(h);
    expect(h.chain.broadcastsOf('sweep')).toHaveLength(1);
  });

  it('the sweep in-flight set is rebuilt at boot from unresolved sweep entries', async () => {
    // mutation: empty at boot → the job is free to be swept again.
    const h = await makeHarness();
    const jobId = await boundJob(h);
    await tick(h); // sweep sent, never mined
    expect(h.chain.broadcastsOf('sweep')).toHaveLength(1);
    const h2 = await h.reboot();
    expect(h2.outbox.inFlight.sweeps.has(jobId.toString())).toBe(true);
    await tick(h2);
    expect(h2.chain.broadcastsOf('sweep')).toHaveLength(1);
  });

  it("a sweep's in-flight entry is taken when the job is CHOSEN: two ticks waiting on a lock held past the 120 s watchdog send one sweep", async () => {
    // mutation: drop both the choice-time entry and the under-lock re-check → two sends.
    const h = await makeHarness();
    await boundJob(h);
    const holder = h.outbox.withVaultLock(() => new Promise((r) => setTimeout(r, 140_000)));
    const tick1 = h.tick();
    await vi.advanceTimersByTimeAsync(121_000); // the watchdog abandons tick 1 (its sweep still waits)
    const tick2 = h.tick();
    await vi.advanceTimersByTimeAsync(200_000);
    await holder;
    await tick1;
    await tick2;
    expect(h.chain.broadcastsOf('sweep')).toHaveLength(1);
    expect(h.alarms.join('\n')).toMatch(/tick watchdog/);
  });

  it('a dropped sweep frees its job for a re-sweep', async () => {
    // mutation: leave the set only on a receipt or a not-Active read → never swept again.
    const h = await makeHarness();
    await boundJob(h);
    h.chain.loseBroadcast = (d) => d.kind === 'sweep';
    await tick(h); // sent and lost; waits 90 s; stays in flight
    expect(h.chain.broadcastsOf('sweep')).toHaveLength(1);
    h.chain.loseBroadcast = undefined;
    const lostNonce = h.chain.broadcastsOf('sweep')[0]!.nonce;
    h.chain.foreign(lostNonce);
    h.chain.mineAll(); // something else took the nonce
    await tick(h); // margin
    h.chain.block += 12;
    await vi.advanceTimersByTimeAsync(10 * MIN);
    await tick(h); // dropped → the same tick re-sweeps
    const sweeps = h.chain.broadcastsOf('sweep');
    expect(sweeps).toHaveLength(2);
    expect(sweeps[1]!.nonce).toBeGreaterThan(lostNonce);
  });
});

describe('crash and pre-deploy entries', () => {
  it('a crash after the append and before the broadcast: at boot the entry is broadcast, mines, and its hold is BOUND', async () => {
    // mutation: judge the hold by its own mirrored hash → released after
    // 10 min (no mirror was written) while its session mines.
    const h = await makeHarness();
    h.chain.hangBroadcast = true;
    void h.open({ idempotencyKey: 'k1' });
    await vi.advanceTimersByTimeAsync(0);
    expect(h.outbox.all()).toHaveLength(1);
    h.crash();
    h.chain.hangBroadcast = false;
    const h2 = await h.reboot();
    expect(h2.ledger.hold('h0')).toMatchObject({ state: 'held' });
    expect(h2.ledger.hold('h0')?.pendingTxHash).toBeUndefined();
    await vi.advanceTimersByTimeAsync(11 * MIN);
    await tick(h2);
    expect(h2.chain.mempool.size).toBe(1); // rebroadcast: the same bytes
    expect(h2.ledger.hold('h0')?.state).toBe('held');
    h2.chain.mineAll();
    await tick(h2);
    expect(h2.ledger.hold('h0')?.state).toBe('bound');
    expect((await h2.idempotency.lookup(USER, 'k1'))?.state).toBe('done');
  });

  it('entries before the migration marker raise an alarm ONCE and are never released or freed', async () => {
    // mutation A: apply the new rules → freed while their session may exist.
    // mutation B: alarm every tick → the pre-migration alarms repeat.
    const h0 = await makeHarness({ migrated: false });
    const gate = h0.deps.gatekeeper;
    const a = await h0.ledger.openHold({ userId: USER, host: HOST, depositMicro: 500_000n }, gate);
    const b = await h0.ledger.openHold({ userId: USER, host: HOST, depositMicro: 500_000n }, gate);
    if (!a.ok || !b.ok) throw new Error('refused');
    await h0.ledger.recordCreatePending(a.holdId, `0x${'aa'.repeat(32)}`); // no receipt, ever
    await h0.idempotency.reserve(USER, 'old-key', 'old-fp'); // no clientAddress
    const h = await h0.reboot({ migrated: true });
    expect(h.ledger.hasMigrationMarker()).toBe(true);
    await vi.advanceTimersByTimeAsync(11 * MIN);
    await tick(h);
    await tick(h);
    expect(h.ledger.hold(a.holdId)?.state).toBe('held');
    expect(h.ledger.hold(b.holdId)?.state).toBe('held');
    expect((await h.idempotency.lookup(USER, 'old-key'))?.state).toBe('pending');
    const count = (re: RegExp) => h.alarms.filter((m) => re.test(m)).length;
    expect(count(/pre-deploy create 0xaaaa.* has no receipt/)).toBe(1);
    expect(count(/predates the outbox and carries no create hash/)).toBe(1);
    expect(count(/reserved before the outbox deploy/)).toBe(1);
  });

  it('a pre-deploy create mined with status 1 but NO creation event stays held (its key pending) with ONE alarm', async () => {
    // mutation: treat it as a revert → the hold is released while a session
    // may exist under an event the service cannot read.
    const h0 = await makeHarness({ migrated: false });
    const hash = `0x${'bb'.repeat(32)}`;
    h0.chain.mined.set(hash, { status: 1, blockNumber: h0.chain.block, logs: [] });
    const e = await h0.ledger.openHold({ userId: USER, host: HOST, depositMicro: 500_000n }, h0.deps.gatekeeper);
    if (!e.ok) throw new Error('refused');
    await h0.ledger.recordCreatePending(e.holdId, hash);
    await h0.idempotency.reserve(USER, 'old-key', 'old-fp'); // pre-deploy: no clientAddress
    const h = await h0.reboot({ migrated: true });
    await vi.advanceTimersByTimeAsync(11 * MIN);
    await tick(h);
    await tick(h);
    expect(h.ledger.hold(e.holdId)?.state).toBe('held');
    expect((await h.idempotency.lookup(USER, 'old-key'))?.state).toBe('pending');
    expect(h.alarms.filter((m) => /pre-deploy create 0xbbbb.* mined with no SessionJobCreatedForModel event/.test(m))).toHaveLength(1);
  });

  it('a pre-deploy create-pending still binds on a receipt, or releases on a revert', async () => {
    const h0 = await makeHarness({ migrated: false });
    const data = ESCROW_INTERFACE.encodeFunctionData('createSessionJobForModelWithToken', [
      HOST, MODEL, USDC, 500_000n, 904n, 3600n, 1000n, 300n,
    ]);
    // Two creates the OLD code sent (outside any outbox): one mines, one reverts.
    const sign = (nonce: number) =>
      h0.chain.wallet.signTransaction({
        type: 2, chainId: h0.chain.chainId, nonce, to: MARKETPLACE, data, value: 0n,
        gasLimit: 100_000n, maxFeePerGas: 1_000_000_000n, maxPriorityFeePerGas: 1n,
      });
    const okRaw = await sign(0);
    await h0.chain.broadcast(okRaw);
    h0.chain.mineAll();
    h0.chain.allowance = 0n;
    const badRaw = await sign(1);
    await h0.chain.broadcast(badRaw);
    h0.chain.mineAll();
    const { Transaction } = await import('ethers');
    const gate = h0.deps.gatekeeper;
    const c = await h0.ledger.openHold({ userId: USER, host: HOST, depositMicro: 500_000n }, gate);
    const d = await h0.ledger.openHold({ userId: USER, host: HOST, depositMicro: 500_000n }, gate);
    if (!c.ok || !d.ok) throw new Error('refused');
    await h0.ledger.recordCreatePending(c.holdId, Transaction.from(okRaw).hash!);
    await h0.ledger.recordCreatePending(d.holdId, Transaction.from(badRaw).hash!);
    const h = await h0.reboot({ migrated: true });
    await tick(h);
    expect(h.ledger.hold(c.holdId)?.state).toBe('bound');
    expect(h.ledger.hold(d.holdId)?.state).toBe('released');
  });
});
