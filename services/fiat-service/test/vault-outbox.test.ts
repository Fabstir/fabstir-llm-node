// D20 item 1 (BM1.2c) — the vault outbox: one send path, one lock, every send
// journalled before broadcast; the create's bounds; outbox resolution and the
// stuck rule. The chain is scripted (test/helpers/scripted-chain.ts); time is
// vitest's fake clock. Each test names the mutation that turns it red.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Interface, Transaction } from 'ethers';
import { makeVaultChain } from '../src/lib/fiat-vault';
import { HOST, MARKETPLACE, MODEL, USDC } from './helpers/scripted-chain';
import { CLIENT, USER, makeHarness, memoryStores, type Harness } from './helpers/vault-harness';

const MIN = 60_000;

/** Run timers forward and report whether `p` settled, without awaiting it forever. */
async function advance<T>(p: Promise<T>, ms: number): Promise<{ done: boolean; value?: T; error?: unknown }> {
  const state: { done: boolean; value?: T; error?: unknown } = { done: false };
  p.then(
    (value) => Object.assign(state, { done: true, value }),
    (error) => Object.assign(state, { done: true, error })
  );
  await vi.advanceTimersByTimeAsync(ms);
  return state;
}

function linesOf(h: Harness): Promise<Array<Record<string, unknown>>> {
  return h.stores.outbox.load().then((lines) => lines.map((l) => JSON.parse(l) as Record<string, unknown>));
}

beforeEach(() => {
  vi.useFakeTimers({ now: new Date('2026-10-08T00:00:00Z') });
});
afterEach(() => {
  vi.useRealTimers();
});

describe('the send path (sendLocked / vaultSend)', () => {
  it('appends {kind, nonce, hash, raw, link} BEFORE the broadcast; a create that throws after reaching the mempool keeps its hold and binds when it mines', async () => {
    // mutation: broadcast first → the broadcast throw lands before the append,
    // the create releases its hold and frees its key while the session mines.
    const h = await makeHarness();
    const seenAtBroadcast: string[] = [];
    h.chain.onBroadcast = async (_raw, d) => {
      if (d.kind !== 'create') return;
      seenAtBroadcast.push(...(await h.stores.outbox.load()));
      throw new Error('the RPC dropped the reply'); // the tx IS in the mempool
    };
    const outcome = await h.open({ idempotencyKey: 'k1' });
    expect(outcome.status).toBe('chain_pending');
    const entry = JSON.parse(seenAtBroadcast[0]!) as Record<string, unknown>;
    expect(entry).toMatchObject({ t: 'send', kind: 'create', nonce: 0, link: 'h0' });
    expect(entry.hash).toBe(Transaction.from(entry.raw as string).hash);
    expect(h.ledger.hold('h0')?.state).toBe('held');
    expect((await h.idempotency.lookup(USER, 'k1'))?.state).toBe('pending');

    h.chain.mineAll();
    await h.tick();
    expect(h.ledger.hold('h0')?.state).toBe('bound');
    expect((await h.idempotency.lookup(USER, 'k1'))?.state).toBe('done');
  });

  it('a throw after the append keeps the hold held and the key pending (chain_pending): a same-key retry opens nothing', async () => {
    // mutation: release on every throw → the retry opens a second session.
    const h = await makeHarness();
    let first = true;
    h.chain.onBroadcast = async (_raw, d) => {
      if (d.kind === 'create' && first) {
        first = false;
        throw new Error('socket hang up');
      }
    };
    expect((await h.open({ idempotencyKey: 'k1' })).status).toBe('chain_pending');
    const retry = await h.open({ idempotencyKey: 'k1' });
    expect(retry.status).toBe('in_flight');
    expect(h.chain.broadcastsOf('create')).toHaveLength(1);
    expect(h.ledger.hold('h0')?.state).toBe('held');
  });

  it('a create whose hold is no longer held aborts BEFORE the append (nothing journalled, nothing broadcast)', async () => {
    // mutation: check after the send → an entry appended and broadcast for a released hold.
    const h = await makeHarness();
    const vault = h.chain.vault();
    const chain = makeVaultChain({ ...vault, outbox: h.outbox });
    await expect(
      chain.createSession({ host: HOST, modelId: MODEL, depositMicro: 500_000n, link: 'h9', assertHeld: () => false })
    ).rejects.toThrow(/no longer held/);
    expect(h.outbox.all()).toHaveLength(0);
    expect(h.chain.broadcasts).toHaveLength(0);
  });

  it('the ETH check counts the scripted L1 data fee: a balance between the two figures aborts before the append', async () => {
    // mutation: L2 gas only → appended, the broadcast refused by the chain
    // (which charges the L1 fee) → chain_pending with an entry nobody can mine.
    const h = await makeHarness();
    const l2 = h.chain.gasLimit * h.chain.maxFeePerGas;
    h.chain.l1FeeWei = 50_000_000_000_000n;
    h.chain.ethBalance = l2 + h.chain.l1FeeWei / 2n;
    const outcome = await h.open({ idempotencyKey: 'k1' });
    expect(outcome.status).toBe('chain_error');
    expect(h.outbox.all()).toHaveLength(0);
    expect(h.chain.broadcasts).toHaveLength(0);
    expect(await h.idempotency.lookup(USER, 'k1')).toBeNull();
    expect(h.ledger.hold('h0')?.state).toBe('released');
  });

  it('ETH below this send plus every unresolved entry: the create aborts before the append, frees its key, and the ETH alarm fires', async () => {
    // mutation: no pre-append ETH check → B is appended and broadcast with no
    // ETH behind the unresolved entries (it mines whenever ETH arrives).
    const h = await makeHarness();
    const maxCost = h.chain.gasLimit * h.chain.maxFeePerGas;
    const a = h.open({ idempotencyKey: 'a' }); // unconfirmed: reserves maxCost
    await vi.advanceTimersByTimeAsync(0);
    expect(h.outbox.unresolved()).toHaveLength(1);
    h.chain.ethBalance = maxCost + maxCost / 2n;
    const b = (await advance(h.open({ idempotencyKey: 'b' }), 5_000)).value;
    expect(b).toMatchObject({ status: 'chain_error' });
    expect(h.outbox.all()).toHaveLength(1);
    expect(await h.idempotency.lookup(USER, 'b')).toBeNull();
    expect(h.alarms.join('\n')).toMatch(/vault ETH floor/);
    h.chain.mineAll();
    expect((await advance(a, 5_000)).value).toMatchObject({ status: 'ok' });
  });

  it('the lock is released when the create broadcast returns, not held through its confirmation', async () => {
    // mutation: hold it through the wait → the second open waits for the
    // first's confirmation (one create broadcast until the first mines).
    const h = await makeHarness();
    const a = h.open({ idempotencyKey: 'a' });
    const b = h.open({ idempotencyKey: 'b' });
    await vi.advanceTimersByTimeAsync(3_000);
    expect(h.chain.broadcastsOf('create').map((x) => x.nonce)).toEqual([0, 1]);
    h.chain.mineAll();
    await vi.advanceTimersByTimeAsync(3_000);
    expect((await a).status).toBe('ok');
    expect((await b).status).toBe('ok');
  });

  it("the create's approve and its in-create retry run through sendLocked inside the create's lock", async () => {
    // mutation: call vaultSend there → the non-reentrant lock deadlocks, the
    // acquisition times out and the open fails.
    const h = await makeHarness();
    h.chain.autoMine = true;
    // First the pre-check's approve (allowance 0), then the in-create retry
    // (a stale read says plenty; the estimate reverts on allowance).
    h.chain.allowance = 0n;
    const first = h.open({ idempotencyKey: 'a' });
    expect((await advance(first, 200_000)).value).toMatchObject({ status: 'ok' });
    h.chain.allowance = 0n;
    h.chain.staleAllowanceRead = 100_000_000n;
    const second = h.open({ idempotencyKey: 'b' });
    expect((await advance(second, 200_000)).value).toMatchObject({ status: 'ok' });
    expect(h.chain.broadcasts.map((x) => `${x.decoded.kind}@${x.nonce}`)).toEqual([
      'approve@0',
      'create@1',
      'approve@2',
      'create@3',
    ]);
  });

  it('a second create while a first is unconfirmed sees the allowance NET of the first deposit and approves first', async () => {
    // mutation: gross allowance → no approve; the second create mines after the
    // first has consumed the allowance and reverts on chain.
    const h = await makeHarness();
    h.chain.allowance = 1_000_000n;
    h.chain.onBroadcast = (_raw, d) => {
      if (d.kind === 'approve' || (d.kind === 'create' && h.chain.broadcastsOf('create').length === 2)) h.chain.mineAll();
    };
    const a = h.open({ idempotencyKey: 'a', depositMicro: 600_000n });
    await vi.advanceTimersByTimeAsync(0);
    const b = h.open({ idempotencyKey: 'b', depositMicro: 600_000n });
    await vi.advanceTimersByTimeAsync(5_000);
    expect((await a).status).toBe('ok');
    expect((await b).status).toBe('ok');
    expect(h.chain.broadcasts.map((x) => x.decoded.kind)).toEqual(['create', 'approve', 'create']);
  });
});

describe('the create request bounds (150 s append start, 170 s deadline, 120 s approve)', () => {
  it('with a lock holder keeping the lock 151 s, a create broadcasts nothing, frees its key and answers chain_error; a sweep gives up, leaves the in-flight set and appends nothing', async () => {
    // mutation: start the 150 s clock at lock acquisition → both proceed
    // after the helper gave up.
    const h = await makeHarness();
    h.chain.sessions.set(77n, 0);
    const holder = h.outbox.withVaultLock(() => new Promise((r) => setTimeout(r, 151_000)));
    const open = h.open({ idempotencyKey: 'k1' });
    const sweep = (await import('../src/lib/settlement-listener'))
      .makeReclaimer(h.outbox, MARKETPLACE)
      .trigger(77n)
      .then(() => 'sent', (e: unknown) => e);
    const openState = await advance(open, 152_000);
    await holder;
    expect(openState.value).toMatchObject({ status: 'chain_error' });
    expect(await h.idempotency.lookup(USER, 'k1')).toBeNull();
    expect(await sweep).toBeInstanceOf(Error);
    expect(h.outbox.inFlight.sweeps.has('77')).toBe(false);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(h.outbox.all()).toHaveLength(0);
    expect(h.chain.broadcasts).toHaveLength(0);
  });

  it('a create unconfirmed at 170 s answers chain_pending, leaves the in-flight set, and R5 resolves it', async () => {
    // mutation: unbounded wait → still waiting at 171 s, the key stays in
    // flight (R5 skips it).
    const h = await makeHarness();
    const open = h.open({ idempotencyKey: 'k1' });
    const state = await advance(open, 171_000);
    expect(state.value).toMatchObject({ status: 'chain_pending' });
    expect(h.outbox.inFlight.holds.size + h.outbox.inFlight.keys.size).toBe(0);
    h.chain.mineAll();
    await h.tick();
    expect(h.ledger.hold('h0')?.state).toBe('bound');
    expect((await h.idempotency.lookup(USER, 'k1'))?.state).toBe('done');
  });

  it("the in-create allowance retry's approve aborts at 120 s, releasing the hold and freeing the key before any create", async () => {
    // mutation: unbounded approve wait → the open (and every open behind the lock) waits for ever.
    const h = await makeHarness();
    h.chain.allowance = 0n;
    h.chain.staleAllowanceRead = 100_000_000n; // the pre-check passes; the estimate reverts
    const open = h.open({ idempotencyKey: 'k1' });
    const state = await advance(open, 125_000);
    expect(state.value).toMatchObject({ status: 'chain_error' });
    expect(h.chain.broadcastsOf('create')).toHaveLength(0);
    expect(h.outbox.unresolved('create')).toHaveLength(0);
    expect(h.ledger.hold('h0')?.state).toBe('released');
    expect(await h.idempotency.lookup(USER, 'k1')).toBeNull();
  });

  it('appended at ≈149 s with a slow broadcast and a slow receipt read, the open still answers chain_pending by 170 s', async () => {
    // mutation: the post-append receipt read bounded by the 20 s RPC timeout
    // alone (read before checking what is left) → answers at ≈188 s, after
    // the helper and nginx gave up at 180 s.
    const h = await makeHarness();
    const holder = h.outbox.withVaultLock(() => new Promise((r) => setTimeout(r, 149_000)));
    h.chain.onBroadcast = () => new Promise((r) => setTimeout(r, 19_000));
    h.chain.onReceipt = () => new Promise((r) => setTimeout(r, 30_000));
    const state = await advance(h.open({ idempotencyKey: 'k1' }), 170_000);
    await holder;
    expect(h.outbox.all()[0]).toMatchObject({ kind: 'create', at: Date.parse('2026-10-08T00:02:29Z') });
    expect(state.value).toMatchObject({ status: 'chain_pending' });
    expect(h.ledger.hold('h0')?.state).toBe('held');
    expect((await h.idempotency.lookup(USER, 'k1'))?.state).toBe('pending');
  });

  it('a slow durable append (≈10 s fsync) at 149 s: the broadcast is bounded by what is left of 170 s, not a fresh 20 s', async () => {
    // mutation: the broadcast bounded by the 20 s RPC timeout alone → a hung
    // broadcast answers at ≈179 s.
    const stores = memoryStores();
    const outboxStore = stores.outbox;
    stores.outbox = {
      load: () => outboxStore.load(),
      append: async (line: string) => {
        if (line.includes('"t":"send"')) await new Promise((r) => setTimeout(r, 10_000));
        return outboxStore.append(line);
      },
    };
    const h = await makeHarness({ stores });
    const holder = h.outbox.withVaultLock(() => new Promise((r) => setTimeout(r, 149_000)));
    h.chain.hangBroadcast = true;
    const state = await advance(h.open({ idempotencyKey: 'k1' }), 170_000);
    await holder;
    expect(h.outbox.all()).toHaveLength(1);
    expect(state.value).toMatchObject({ status: 'chain_pending' });
    expect(h.ledger.hold('h0')?.state).toBe('held');
  });

  it('a hung registry price read ends as chain_error inside the window (no hold, key freed), not after the helper gave up', async () => {
    // mutation: unbounded price read (ethers' default is 300 s) → no answer
    // by 180 s; the helper and nginx give up first.
    const h = await makeHarness();
    h.deps.chain.modelPrice = () => new Promise<bigint>(() => {});
    const state = await advance(h.open({ idempotencyKey: 'k1' }), 25_000);
    expect(state.value).toMatchObject({ status: 'chain_error', message: expect.stringMatching(/price read/) });
    expect(h.ledger.allHolds()).toHaveLength(0);
    expect(await h.idempotency.lookup(USER, 'k1')).toBeNull();
    expect(h.outbox.inFlight.keys.size).toBe(0);
    expect(h.chain.broadcasts).toHaveLength(0);
  });

  it('a hung outbox fsync holding the lock: the next opens time out on the lock with ONE alarm per episode; a new hang after a successful acquisition alarms again', async () => {
    // mutation A: no alarm on a lock timeout → a hung journal shows only as
    //   502 chain_error, with nothing unresolved to trip the stuck alarm.
    // mutation B: no reset after a successful acquisition → the second
    //   episode is silent (one alarm in total).
    const stores = memoryStores();
    const inner = stores.outbox;
    const hangs: Array<Promise<void>> = [];
    stores.outbox = {
      load: () => inner.load(),
      append: async (line: string) => {
        const gate = line.includes('"t":"send"') ? hangs.shift() : undefined;
        if (gate) await gate; // the fsync hangs until the gate opens
        return inner.append(line);
      },
    };
    const h = await makeHarness({ stores });
    h.chain.autoMine = true;
    const lockAlarms = () => h.alarms.filter((m) => /vault lock not acquired/.test(m)).length;

    // Episode 1: A hangs in its fsync, holding the lock; B and C time out.
    let release1!: () => void;
    hangs.push(new Promise<void>((r) => (release1 = r)));
    const a = h.open({ idempotencyKey: 'a' });
    await vi.advanceTimersByTimeAsync(0);
    expect((await advance(h.open({ idempotencyKey: 'b' }), 151_000)).value).toMatchObject({ status: 'chain_error' });
    expect((await advance(h.open({ idempotencyKey: 'c' }), 151_000)).value).toMatchObject({ status: 'chain_error' });
    expect(lockAlarms()).toBe(1);

    // The holder lets go: A's entry lands (past its deadline, unbroadcast →
    // chain_pending), resolution rebroadcasts it, it mines, and D opens.
    release1();
    expect((await advance(a, 1_000)).value).toMatchObject({ status: 'chain_pending' });
    await h.tick();
    expect(h.outbox.unresolved()).toHaveLength(0);
    expect((await advance(h.open({ idempotencyKey: 'd' }), 5_000)).value).toMatchObject({ status: 'ok' });
    expect(lockAlarms()).toBe(1);

    // Episode 2: E hangs; F times out → a SECOND alarm.
    let release2!: () => void;
    hangs.push(new Promise<void>((r) => (release2 = r)));
    const e = h.open({ idempotencyKey: 'e' });
    await vi.advanceTimersByTimeAsync(0);
    expect((await advance(h.open({ idempotencyKey: 'f' }), 151_000)).value).toMatchObject({ status: 'chain_error' });
    expect(lockAlarms()).toBe(2);
    release2();
    await advance(e, 1_000);
  });

  it("a failed APPROVE append (the create's allowance top-up) is not the create's: chain_error, hold released, key freed", async () => {
    // mutation: treat any OutboxAppendFailed as the create's → chain_pending
    //   with the hold held and the key pending, though no create was signed.
    const stores = memoryStores();
    const inner = stores.outbox;
    stores.outbox = {
      load: () => inner.load(),
      append: async (line: string) => {
        if (line.includes('"kind":"approve"')) throw new Error('EIO: fsync failed');
        return inner.append(line);
      },
    };
    const h = await makeHarness({ stores });
    h.chain.allowance = 0n; // the pre-check approves first
    const outcome = await advance(h.open({ idempotencyKey: 'k1' }), 5_000);
    expect(outcome.value).toMatchObject({ status: 'chain_error' });
    expect(h.ledger.hold('h0')?.state).toBe('released');
    expect(await h.idempotency.lookup(USER, 'k1')).toBeNull();
    expect(h.chain.broadcasts).toHaveLength(0);
  });

  /** A harness whose FIRST outbox send append throws: `onDisk` = its bytes
   *  reached the file first (a failed fsync), else nothing was written. */
  async function failedAppendHarness(onDisk: boolean) {
    const stores = memoryStores();
    const inner = stores.outbox;
    let failed = false;
    stores.outbox = {
      load: () => inner.load(),
      append: async (line: string) => {
        const fail = !failed && line.includes('"t":"send"');
        if (fail) failed = true;
        if (!fail || onDisk) await inner.append(line);
        if (fail) throw new Error('EIO: fsync failed');
      },
    };
    const h = await makeHarness({ stores });
    return h;
  }

  it('a FAILED outbox append (the fsync threw) is ambiguous: chain_pending, hold held, key pending; then vault_stuck and nothing more sent until restart', async () => {
    // mutation A: treat it as before the append → chain_error, the hold
    //   released and the key freed while the line may be on disk.
    // mutation B: keep sending → the next create is signed at the same nonce.
    const h = await failedAppendHarness(true);
    h.chain.autoMine = true;
    const first = await advance(h.open({ idempotencyKey: 'a' }), 5_000);
    expect(first.value).toMatchObject({ status: 'chain_pending' });
    expect(h.ledger.hold('h0')?.state).toBe('held');
    expect((await h.idempotency.lookup(USER, 'a'))?.state).toBe('pending');
    expect(h.alarms.join('\n')).toMatch(/outbox journal write failed/);
    const second = await advance(h.open({ idempotencyKey: 'b' }), 5_000);
    expect(second.value).toEqual({ status: 'vault_stuck' });
    expect(h.chain.broadcasts).toHaveLength(0);
    // The sweeper's path refuses too.
    await expect(h.outbox.vaultSend('sweep', { to: MARKETPLACE, data: '0x' }, '77')).rejects.toThrow(/refused until restart/);
    expect(h.chain.broadcasts).toHaveLength(0);
  });

  it('while the failed-append state lasts, R5 never applies the 10-min unlinked release (the index may lack a line that is on disk)', async () => {
    // mutation: apply the unlinked rule anyway → released at 15 min while the
    // create's line may be on disk (and be rebroadcast after a restart).
    const h = await failedAppendHarness(true);
    expect((await advance(h.open({ idempotencyKey: 'a' }), 5_000)).value).toMatchObject({ status: 'chain_pending' });
    await vi.advanceTimersByTimeAsync(15 * MIN);
    await h.tick();
    expect(h.ledger.hold('h0')?.state).toBe('held');
    expect((await h.idempotency.lookup(USER, 'a'))?.state).toBe('pending');
  });

  it('after a restart the replay decides: the line on disk → linked, rebroadcast, mined and bound', async () => {
    const h = await failedAppendHarness(true);
    expect((await advance(h.open({ idempotencyKey: 'a' }), 5_000)).value).toMatchObject({ status: 'chain_pending' });
    await vi.advanceTimersByTimeAsync(15 * MIN);
    const h2 = await h.reboot();
    expect(h2.outbox.all()).toEqual([expect.objectContaining({ kind: 'create', nonce: 0, link: 'h0' })]);
    await h2.tick(); // nothing occupies nonce 0: the same bytes are rebroadcast
    expect(h2.chain.broadcastsOf('create')).toHaveLength(1);
    h2.chain.mineAll();
    await h2.tick();
    expect(h2.ledger.hold('h0')?.state).toBe('bound');
    expect((await h2.idempotency.lookup(USER, 'a'))?.state).toBe('done');
  });

  it('after a restart the replay decides: the line absent → nothing was ever sent, the hold is released once 10 min old and its key freed', async () => {
    const h = await failedAppendHarness(false);
    expect((await advance(h.open({ idempotencyKey: 'a' }), 5_000)).value).toMatchObject({ status: 'chain_pending' });
    const h2 = await h.reboot();
    expect(h2.outbox.all()).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(5 * MIN);
    await h2.tick();
    expect(h2.ledger.hold('h0')?.state).toBe('held');
    await vi.advanceTimersByTimeAsync(6 * MIN);
    await h2.tick();
    expect(h2.ledger.hold('h0')?.state).toBe('released');
    expect(await h2.idempotency.lookup(USER, 'a')).toBeNull();
    expect(h2.chain.broadcasts).toHaveLength(0);
  });

  it("the allowance pre-check's approve aborts at 120 s before any create", async () => {
    // mutation: unbounded approve wait → never answers.
    const h = await makeHarness();
    h.chain.allowance = 0n;
    const open = h.open({ idempotencyKey: 'k1' });
    const state = await advance(open, 125_000);
    expect(state.value).toMatchObject({ status: 'chain_error' });
    expect(h.chain.broadcasts.map((b) => b.decoded.kind)).toEqual(['approve']);
    expect(h.ledger.hold('h0')?.state).toBe('released');
  });
});

describe('nonces', () => {
  it('after a restart the nonce comes from the replayed outbox when the RPC lags', async () => {
    // mutation: RPC nonce only → two transactions at N.
    const h = await makeHarness();
    void h.open({ idempotencyKey: 'a' });
    await vi.advanceTimersByTimeAsync(0);
    const h2 = await h.reboot();
    h2.chain.pendingOverride = 0; // a lagging replica: it has not seen nonce 0
    void h2.open({ idempotencyKey: 'b' });
    await vi.advanceTimersByTimeAsync(0);
    expect(h2.chain.broadcastsOf('create').map((b) => b.nonce)).toEqual([0, 1]);
    expect(h2.chain.replaced).toEqual([]);
  });

  it("after the last entry resolved mined, a lagging RPC's pending nonce N still yields N+1", async () => {
    // mutation: highest UNRESOLVED nonce only → two transactions at N.
    const h = await makeHarness();
    h.chain.autoMine = true;
    expect((await h.open({ idempotencyKey: 'a' })).status).toBe('ok');
    expect(h.outbox.unresolved()).toHaveLength(0);
    h.chain.pendingOverride = 0;
    h.chain.latestOverride = 0;
    h.chain.autoMine = false;
    void h.open({ idempotencyKey: 'b' });
    await vi.advanceTimersByTimeAsync(0);
    expect(h.chain.broadcastsOf('create').map((b) => b.nonce)).toEqual([0, 1]);
  });
});

describe('outbox resolution', () => {
  it('a rebroadcast sends the entry\'s own raw bytes and appends nothing', async () => {
    // mutation: route it through vaultSend → a new nonce and a second entry,
    // or a deadlock under the per-entry lock.
    const h = await makeHarness();
    h.chain.loseBroadcast = (d) => d.kind === 'create';
    const open = h.open({ idempotencyKey: 'k1' });
    expect((await advance(open, 171_000)).value).toMatchObject({ status: 'chain_pending' });
    const entry = h.outbox.all()[0]!;
    h.chain.loseBroadcast = undefined;
    const tick = h.tick();
    await vi.advanceTimersByTimeAsync(130_000);
    await tick;
    const last = h.chain.broadcasts[h.chain.broadcasts.length - 1]!;
    expect(last.raw).toBe(entry.raw);
    expect(h.chain.broadcasts).toHaveLength(2);
    expect((await linesOf(h)).filter((l) => l.t === 'send')).toHaveLength(1);
    h.chain.mineAll();
    await h.tick();
    expect(h.ledger.hold('h0')?.state).toBe('bound');
  });

  it('a lost approve at N, then a create at N+1: resolution rebroadcasts N and both mine', async () => {
    // mutation: rebroadcast creates only → the create stays unmined behind the gap
    // and its key pending for ever.
    const h = await makeHarness();
    h.chain.loseBroadcast = (d) => d.kind === 'approve';
    const data = new Interface(['function approve(address,uint256)']).encodeFunctionData('approve', [MARKETPLACE, 2_000_000n]);
    const approve = await h.outbox.vaultSend('approve', { to: USDC, data }, 'allowance');
    expect(approve).toMatchObject({ nonce: 0 });
    const open = h.open({ idempotencyKey: 'k1' });
    expect((await advance(open, 171_000)).value).toMatchObject({ status: 'chain_pending' });
    expect(h.chain.broadcastsOf('create')[0]!.nonce).toBe(1);
    h.chain.loseBroadcast = undefined;
    await h.tick(); // rebroadcasts the approve at 0, the gap closes
    expect(h.chain.mineAll()).toBe(2);
    await h.tick();
    expect(h.ledger.hold('h0')?.state).toBe('bound');
    expect((await h.idempotency.lookup(USER, 'k1'))?.state).toBe('done');
  });

  it('no rebroadcast while L ≤ N < P (another transaction sits at N)', async () => {
    // mutation: rebroadcast anyway → the scripted mempool replaces what sits at N.
    const h = await makeHarness();
    h.chain.loseBroadcast = (d) => d.kind === 'create';
    const open = h.open({ idempotencyKey: 'k1' });
    expect((await advance(open, 171_000)).value).toMatchObject({ status: 'chain_pending' });
    h.chain.loseBroadcast = undefined;
    const occupant = h.chain.foreign(0); // e.g. the runbook's cancel at N
    await h.tick();
    expect(h.chain.broadcasts).toHaveLength(1); // the original broadcast only
    expect(h.chain.mempool.get(0)).toBe(occupant);
    expect(h.chain.replaced).toEqual([]);
  });

  it('a dropped create is released only after a FRESH read once BOTH 12 blocks AND 10 min have passed', async () => {
    // mutation A: decide on the read made before the margin → a create that
    // mined during the margin is released while its session exists.
    const h = await makeHarness();
    const open = h.open({ idempotencyKey: 'k1' });
    await vi.advanceTimersByTimeAsync(0);
    const hash = h.chain.broadcastsOf('create')[0]!.hash;
    h.chain.hiddenReceipts.add(hash); // a lagging replica
    h.chain.mineAll();
    expect((await advance(open, 171_000)).value).toMatchObject({ status: 'chain_pending' });
    await h.tick(); // L > N, no receipt: the margin starts
    expect(h.outbox.all()[0]!.margin).toBeDefined();
    h.chain.hiddenReceipts.delete(hash); // the replica catches up during the margin
    h.chain.block += 13;
    await vi.advanceTimersByTimeAsync(11 * MIN);
    await h.tick();
    // Never resolved `dropped`: the verdict came from the fresh read.
    expect((await linesOf(h)).filter((l) => l.t === 'resolve').map((l) => l.outcome)).toEqual(['mined']);
    expect(h.outbox.all()[0]!.outcome).toBe('mined');
    expect(h.ledger.hold('h0')?.state).toBe('bound');
  });

  it('blocks alone never decide: a replica lagging 13 blocks for a minute does not release a mined create', async () => {
    // mutation B: blocks only → released while its session exists.
    const h = await makeHarness();
    const open = h.open({ idempotencyKey: 'k1' });
    await vi.advanceTimersByTimeAsync(0);
    const hash = h.chain.broadcastsOf('create')[0]!.hash;
    h.chain.hiddenReceipts.add(hash);
    h.chain.mineAll();
    expect((await advance(open, 171_000)).value).toMatchObject({ status: 'chain_pending' });
    await h.tick();
    h.chain.block += 13;
    await vi.advanceTimersByTimeAsync(MIN);
    await h.tick();
    expect(h.outbox.all()[0]!.outcome).toBeUndefined();
    expect(h.ledger.hold('h0')?.state).toBe('held');
    h.chain.hiddenReceipts.delete(hash);
    await h.tick();
    expect(h.ledger.hold('h0')?.state).toBe('bound');
  });

  it('a truly dropped create IS released (and its key freed) after the margin, and a reverted one at once', async () => {
    const h = await makeHarness();
    h.chain.loseBroadcast = (d) => d.kind === 'create';
    const open = h.open({ idempotencyKey: 'k1' });
    expect((await advance(open, 171_000)).value).toMatchObject({ status: 'chain_pending' });
    h.chain.loseBroadcast = undefined;
    h.chain.foreign(0);
    h.chain.mineAll(); // a foreign transaction took nonce 0: L > N for ever
    await h.tick();
    h.chain.block += 12;
    await vi.advanceTimersByTimeAsync(10 * MIN);
    await h.tick();
    expect(h.outbox.all()[0]!.outcome).toBe('dropped');
    expect(h.ledger.hold('h0')?.state).toBe('released');
    expect(await h.idempotency.lookup(USER, 'k1')).toBeNull();

    // Reverted: released as soon as the receipt shows it, no margin.
    h.chain.onBroadcast = (_raw, d) => {
      if (d.kind === 'create') h.chain.allowance = 0n; // consumed before it mines
    };
    const r = h.open({ idempotencyKey: 'k2' });
    await vi.advanceTimersByTimeAsync(0);
    h.chain.mineAll();
    expect((await advance(r, 5_000)).value).toMatchObject({ status: 'chain_pending' });
    await h.tick();
    expect(h.ledger.hold('h1')?.state).toBe('released');
    expect(await h.idempotency.lookup(USER, 'k2')).toBeNull();
  });

  it('resolution holds the lock only for reads and rebroadcast, never across a margin', async () => {
    // mutation: sleep through the margin under the lock → a concurrent open
    // passes its 150 s bound and fails.
    const h = await makeHarness();
    h.chain.loseBroadcast = (d) => d.kind === 'create';
    const first = h.open({ idempotencyKey: 'a' });
    expect((await advance(first, 171_000)).value).toMatchObject({ status: 'chain_pending' });
    h.chain.loseBroadcast = undefined;
    h.chain.foreign(0);
    h.chain.mineAll();
    const tick = h.tick(); // records the margin
    h.chain.autoMine = true;
    const second = h.open({ idempotencyKey: 'b' });
    expect((await advance(second, 160_000)).value).toMatchObject({ status: 'ok' });
    await vi.advanceTimersByTimeAsync(11 * MIN);
    await tick;
  });
});

describe('the stuck rule', () => {
  it('an entry whose every broadcast is rejected raises the stuck alarm 30 min after its `at`, also across a restart, and opens answer vault_stuck', async () => {
    // mutation: stamp the time only on an accepted broadcast → never alarms.
    const h = await makeHarness();
    h.chain.rejectBroadcasts = true;
    expect((await h.open({ idempotencyKey: 'a' })).status).toBe('chain_pending');
    await vi.advanceTimersByTimeAsync(20 * MIN);
    const h2 = await h.reboot();
    await h2.tick();
    expect(h2.outbox.stuckMessage()).toBeNull();
    await vi.advanceTimersByTimeAsync(11 * MIN);
    await h2.tick();
    expect(h2.outbox.stuckMessage()).toMatch(/stuck/);
    expect(h2.alarms.join('\n')).toMatch(/vault outbox stuck/);
    expect((await h2.open({ idempotencyKey: 'b' })).status).toBe('vault_stuck');
    expect(h2.chain.broadcastsOf('create').every((b) => b.nonce === 0)).toBe(true);
  });

  it('a stuck outbox alarms and refuses opens both when the lowest entry is rebroadcast for 30 min and when the mempool holds it', async () => {
    // mutation: count only rebroadcasts → the held-in-mempool case never alarms.
    const a = await makeHarness();
    a.chain.loseBroadcast = () => true;
    void a.open({ idempotencyKey: 'a' });
    await vi.advanceTimersByTimeAsync(31 * MIN);
    await a.tick();
    expect(a.outbox.stuckMessage()).toMatch(/rebroadcast/);
    expect((await a.open({ idempotencyKey: 'a2' })).status).toBe('vault_stuck');

    const b = await makeHarness();
    void b.open({ idempotencyKey: 'b' }); // in the mempool, never mined (a fee below the base fee)
    await vi.advanceTimersByTimeAsync(31 * MIN);
    await b.tick();
    expect(b.outbox.stuckMessage()).toMatch(/held in the mempool/);
    expect((await b.open({ idempotencyKey: 'b2' })).status).toBe('vault_stuck');
    // It clears once the entry mines.
    b.chain.mineAll();
    await b.tick();
    expect(b.outbox.stuckMessage()).toBeNull();
  });

  it('with the latest nonce below the lowest outbox nonce, the stuck alarm names the gap', async () => {
    // mutation: report only N → the runbook's cancel at N queues behind the gap.
    const h = await makeHarness();
    h.chain.foreign(0);
    h.chain.foreign(1); // old-code transactions in the pool when the create was sent
    void h.open({ idempotencyKey: 'a' });
    await vi.advanceTimersByTimeAsync(0);
    expect(h.chain.broadcastsOf('create')[0]!.nonce).toBe(2);
    h.chain.evict(0);
    h.chain.evict(1); // ... and then dropped
    await vi.advanceTimersByTimeAsync(31 * MIN);
    await h.tick();
    expect(h.outbox.stuckMessage()).toMatch(/nonce gap 0\.\.1/);
  });
});

describe('the every-boot nonce wait', () => {
  it('a restart while the boot nonce wait is running keeps refusing opens until latest = pending', async () => {
    // mutation: wait only on the first boot (an empty journal) → an open
    // accepted after the restart.
    const h0 = await makeHarness();
    h0.chain.autoMine = true;
    expect((await h0.open({ idempotencyKey: 'a' })).status).toBe('ok');
    h0.chain.autoMine = false;
    h0.chain.foreign(1); // a transaction the outbox does not own
    const h1 = await h0.reboot();
    expect((await advance(h1.open({ idempotencyKey: 'b' }), 1_000)).value).toMatchObject({ status: 'vault_stuck' });
    const h2 = await h1.reboot();
    expect(h2.outbox.bootWaiting()).toBe(true);
    expect((await advance(h2.open({ idempotencyKey: 'c' }), 1_000)).value).toMatchObject({ status: 'vault_stuck' });
    expect(h2.alarms.join('\n')).toMatch(/boot nonce wait/);
    h2.chain.mineAll();
    await h2.tick();
    h2.chain.autoMine = true;
    expect((await h2.open({ idempotencyKey: 'd' })).status).toBe('ok');
  });

  it("a journal write that throws inside the key's claim leaves no trace: the open throws as before, and the same key then opens normally", async () => {
    // mutation: no cleanup (the key left in the in-flight set, or the
    // in-memory reserved row kept) → every retry answers in_flight until restart.
    const stores = memoryStores();
    const idem = stores.idempotency;
    let failNext = false;
    stores.idempotency = {
      load: () => idem.load(),
      append: async (line: string) => {
        if (failNext) {
          failNext = false;
          throw new Error('EIO: journal write failed');
        }
        return idem.append(line);
      },
    };
    const h = await makeHarness({ stores });
    h.chain.autoMine = true;
    failNext = true;
    await expect(h.open({ idempotencyKey: 'k1' })).rejects.toThrow(/EIO/);
    expect(h.outbox.inFlight.keys.size).toBe(0);
    expect(await h.idempotency.lookup(USER, 'k1')).toBeNull();
    expect(h.ledger.allHolds()).toHaveLength(0);
    expect(h.chain.broadcasts).toHaveLength(0);
    const retry = await h.open({ idempotencyKey: 'k1' });
    expect(retry.status).toBe('ok');
    expect(h.chain.broadcastsOf('create')).toHaveLength(1);
    expect((await h.idempotency.lookup(USER, 'k1'))?.state).toBe('done');
  });

  it('two same-key opens interleaved across a slow boot nonce wait: exactly one create, the other in_flight', async () => {
    // mutation: no re-check of the key after the last await before the
    // reserve → both reserve, both open a hold, two creates for one key.
    const h = await makeHarness({ keepBootWait: true });
    h.chain.autoMine = true;
    const getNonce = h.chain.getNonce.bind(h.chain);
    h.chain.getNonce = (tag) => new Promise((r) => setTimeout(() => r(getNonce(tag)), 20));
    const a = h.open({ idempotencyKey: 'k' });
    await vi.advanceTimersByTimeAsync(0); // A is inside openRefusal's first RPC read
    const b = h.open({ idempotencyKey: 'k' });
    const [ra, rb] = await Promise.all([advance(a, 5_000), advance(b, 5_000)]);
    expect(h.chain.broadcastsOf('create')).toHaveLength(1);
    expect([ra.value?.status, rb.value?.status].sort()).toEqual(['in_flight', 'ok']);
    expect(h.ledger.allHolds()).toHaveLength(1);
    expect((await h.idempotency.lookup(USER, 'k'))?.state).toBe('done');
  });
});

void CLIENT;
