// FC1.3 — the settlement listener: watches SessionCompleted / SessionTimedOut
// (keying userRefund) and RefundCreditedToDeposit (pull-pattern arrival) for
// vault-paid sessions, credits the refund to the owning user, and treats
// SessionCompletedBy as a cross-check. Idempotent by construction (the
// ledger's per-jobId settle guard), so replays and overlapping scans are safe.
import { describe, expect, it } from 'vitest';
import {
  SETTLEMENT_INTERFACE,
  applySettlementEvents,
  parseSettlementLog,
  startSettlementListener,
  type SettlementEvent,
} from '../src/lib/settlement-listener';
import { CreditsLedger, MemoryLedgerStore } from '../src/lib/ledger';
import { makeGatekeeper } from '../src/lib/gatekeeper';

const HOST = '0xabcd000000000000000000000000000000000001';
const VAULT = '0x8ba1f109551bD432803012645Ac136ddd64DBA72';
const USDC = '0x00000000000000000000000000000000000000cc';

const gate = makeGatekeeper({
  allowedHosts: [HOST],
  maxDepositPerSessionMicro: 2_000_000n,
  maxDailySpendMicro: 10_000_000n,
  maxOpensPerMinute: 10,
});

const DEPOSIT = 500_000n;
const REFUND = 299_217n;

async function ledgerWithBoundJob(jobId: bigint) {
  const ledger = await CreditsLedger.open(new MemoryLedgerStore());
  await ledger.purchase('user-1', 1_000_000n, 'evt_1');
  const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
  if (!open.ok) throw new Error('open refused');
  await ledger.bindSession(open.holdId, jobId);
  return ledger;
}

const completed = (jobId: bigint, userRefund: bigint, blockNumber = 10): SettlementEvent => ({
  kind: 'completed',
  jobId,
  userRefund,
  blockNumber,
});

describe('applySettlementEvents', () => {
  it('SessionCompleted credits exactly userRefund to the owning user', async () => {
    const ledger = await ledgerWithBoundJob(818n);
    const alarms: string[] = [];
    const result = await applySettlementEvents(ledger, [completed(818n, REFUND)], (m) => alarms.push(m));
    expect(result.settled).toBe(1);
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n - (DEPOSIT - REFUND));
    expect(alarms).toEqual([]);
  });

  it('SessionTimedOut with a full refund reverses the hold exactly', async () => {
    const ledger = await ledgerWithBoundJob(819n);
    await applySettlementEvents(
      ledger,
      [{ kind: 'timedout', jobId: 819n, userRefund: DEPOSIT, blockNumber: 11 }],
      () => {}
    );
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n);
  });

  it('RefundCreditedToDeposit (pull-pattern) settles through the same idempotent path', async () => {
    const ledger = await ledgerWithBoundJob(820n);
    await applySettlementEvents(
      ledger,
      [{ kind: 'refund-credited', jobId: 820n, amount: REFUND, depositor: VAULT, blockNumber: 12 }],
      () => {}
    );
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n - (DEPOSIT - REFUND));
    // The push event for the same settlement later is a no-op.
    const again = await applySettlementEvents(ledger, [completed(820n, REFUND)], () => {});
    expect(again.settled).toBe(0);
  });

  it('a replayed batch is a no-op', async () => {
    const ledger = await ledgerWithBoundJob(821n);
    await applySettlementEvents(ledger, [completed(821n, REFUND)], () => {});
    const replay = await applySettlementEvents(ledger, [completed(821n, REFUND)], () => {});
    expect(replay.settled).toBe(0);
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n - (DEPOSIT - REFUND));
  });

  it('events for jobs the ledger does not know are skipped silently (not our sessions)', async () => {
    const ledger = await ledgerWithBoundJob(822n);
    const alarms: string[] = [];
    const result = await applySettlementEvents(ledger, [completed(999n, 123n)], (m) => alarms.push(m));
    expect(result.settled).toBe(0);
    expect(alarms).toEqual([]);
  });

  it('SessionCompletedBy arriving first settles; a later amount mismatch raises an alarm, never a second credit', async () => {
    const ledger = await ledgerWithBoundJob(823n);
    const alarms: string[] = [];
    await applySettlementEvents(
      ledger,
      [{ kind: 'completed-by', jobId: 823n, refundAmount: REFUND, blockNumber: 13 }],
      (m) => alarms.push(m)
    );
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n - (DEPOSIT - REFUND));
    expect(alarms).toEqual([]);

    await applySettlementEvents(ledger, [completed(823n, REFUND + 1n)], (m) => alarms.push(m));
    expect(alarms).toHaveLength(1);
    expect(alarms[0]).toMatch(/823/);
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n - (DEPOSIT - REFUND));
  });

  it('a poison event (refund above deposit) alarms and does not wedge later events', async () => {
    const ledger = await ledgerWithBoundJob(824n);
    const other = await CreditsLedger.open(new MemoryLedgerStore());
    void other;
    const alarms: string[] = [];
    const result = await applySettlementEvents(
      ledger,
      [completed(824n, DEPOSIT + 1n, 14), completed(824n, REFUND, 15)],
      (m) => alarms.push(m)
    );
    expect(alarms).toHaveLength(1);
    expect(result.settled).toBe(1); // the sane event after the poison one still lands
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n - (DEPOSIT - REFUND));
  });
});

describe('ledger.refundForJob (cross-check accessor)', () => {
  it('is undefined before settlement and carries the exact refund after', async () => {
    const ledger = await ledgerWithBoundJob(830n);
    expect(ledger.refundForJob(830n)).toBeUndefined();
    await ledger.settle(830n, REFUND);
    expect(ledger.refundForJob(830n)).toBe(REFUND);
  });
});

describe('parseSettlementLog (real event shapes, no chain)', () => {
  it('decodes all four settlement events from encoded logs', () => {
    const enc = (name: string, args: unknown[]) => {
      const log = SETTLEMENT_INTERFACE.encodeEventLog(name, args);
      return parseSettlementLog({ topics: log.topics, data: log.data, blockNumber: 42 });
    };
    expect(enc('SessionCompleted', [818n, 200_783n, 180_705n, REFUND])).toEqual(
      completed(818n, REFUND, 42)
    );
    expect(enc('SessionTimedOut', [819n, 0n, DEPOSIT])).toEqual({
      kind: 'timedout',
      jobId: 819n,
      userRefund: DEPOSIT,
      blockNumber: 42,
    });
    expect(enc('RefundCreditedToDeposit', [820n, VAULT, REFUND, USDC])).toEqual({
      kind: 'refund-credited',
      jobId: 820n,
      amount: REFUND,
      depositor: VAULT,
      blockNumber: 42,
    });
    expect(enc('SessionCompletedBy', [821n, HOST, 200_783n, 180_705n, REFUND])).toEqual({
      kind: 'completed-by',
      jobId: 821n,
      refundAmount: REFUND,
      blockNumber: 42,
    });
  });

  it('returns null for unrelated logs', () => {
    expect(
      parseSettlementLog({ topics: [`0x${'00'.repeat(32)}`], data: '0x', blockNumber: 1 })
    ).toBeNull();
  });
});

describe('startSettlementListener (manual ticks, fake source + cursor)', () => {
  function fakeSource(batches: Record<string, SettlementEvent[]>, latest: () => number) {
    const queries: Array<[number, number]> = [];
    return {
      queries,
      source: {
        latestBlock: async () => latest(),
        query: async (from: number, to: number) => {
          queries.push([from, to]);
          const out: SettlementEvent[] = [];
          for (let b = from; b <= to; b++) out.push(...(batches[String(b)] ?? []));
          return out;
        },
      },
    };
  }

  function memoryCursor(initial?: number) {
    let value = initial;
    return {
      get: () => value,
      cursor: {
        load: async () => value,
        save: async (block: number) => {
          value = block;
        },
      },
    };
  }

  it('processes new blocks, advances the cursor past them, and replays nothing', async () => {
    const ledger = await ledgerWithBoundJob(840n);
    let latest = 105;
    const { source, queries } = fakeSource({ '101': [completed(840n, REFUND, 101)] }, () => latest);
    const { get, cursor } = memoryCursor();
    const listener = startSettlementListener({
      ledger,
      source,
      cursor,
      fromBlock: 100,
      onAlarm: () => {},
      manual: true,
    });

    await listener.tick();
    expect(queries).toEqual([[100, 105]]);
    expect(get()).toBe(106);
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n - (DEPOSIT - REFUND));

    await listener.tick(); // nothing new
    expect(queries).toHaveLength(1);

    latest = 110;
    await listener.tick();
    expect(queries[1]).toEqual([106, 110]);
    await listener.stop();
  });

  // The wedged-listener bug (2026-07-26, live on fabstirserv1 for three days).
  // One query spanned the whole gap, and the cursor only advanced after a batch
  // applied. So once the gap passed the provider's cap the range could only
  // grow: every tick failed on the same range and the listener never recovered.
  // It surfaced as a solvency-breach alarm — settlements were never reconciled,
  // so the ledger looked unbacked and card-paid sessions were refused.
  it('pages a gap wider than the provider cap instead of wedging on it', async () => {
    const CAP = 2000;
    const ledger = await ledgerWithBoundJob(842n);
    const { get, cursor } = memoryCursor(100);
    const queries: Array<[number, number]> = [];
    const source = {
      latestBlock: async () => 100 + 5 * CAP,
      query: async (from: number, to: number) => {
        // Exactly what Base's public RPC does: -32602 over 2000 blocks.
        if (to - from + 1 > CAP) throw new Error('query exceeds max block range 2000');
        queries.push([from, to]);
        return [];
      },
    };
    const listener = startSettlementListener({
      ledger,
      source,
      cursor,
      fromBlock: 100,
      onAlarm: () => {},
      manual: true,
    });

    await listener.tick();

    expect(queries.length).toBeGreaterThan(1);
    for (const [from, to] of queries) expect(to - from + 1).toBeLessThanOrEqual(CAP);
    expect(get()).toBe(100 + 5 * CAP + 1); // fully caught up in one tick
    await listener.stop();
  });

  it('bounds pages per tick and resumes from the persisted cursor', async () => {
    const ledger = await ledgerWithBoundJob(843n);
    const { get, cursor } = memoryCursor(0);
    const queries: Array<[number, number]> = [];
    const source = {
      latestBlock: async () => 10_000,
      query: async (from: number, to: number) => {
        queries.push([from, to]);
        return [];
      },
    };
    const listener = startSettlementListener({
      ledger,
      source,
      cursor,
      fromBlock: 0,
      onAlarm: () => {},
      manual: true,
      maxBlockSpan: 1000,
      maxChunksPerTick: 3,
    });

    await listener.tick();
    expect(queries).toHaveLength(3);
    expect(get()).toBe(3000); // progress banked mid-catch-up

    await listener.tick();
    expect(queries).toHaveLength(6);
    expect(queries[3]).toEqual([3000, 3999]);
    await listener.stop();
  });

  it('keeps the ground it gained when a later page fails', async () => {
    const ledger = await ledgerWithBoundJob(844n);
    const { get, cursor } = memoryCursor(0);
    const alarms: string[] = [];
    const source = {
      latestBlock: async () => 5000,
      query: async (from: number) => {
        if (from >= 2000) throw new Error('rpc blipped');
        return [];
      },
    };
    const listener = startSettlementListener({
      ledger,
      source,
      cursor,
      fromBlock: 0,
      onAlarm: (m: string) => alarms.push(m),
      manual: true,
      maxBlockSpan: 1000,
    });

    await listener.tick();

    // Without per-page saves a mid-catch-up failure would rewind to 0 forever.
    expect(get()).toBe(2000);
    expect(alarms.some((a) => a.includes('tick failed'))).toBe(true);
    await listener.stop();
  });

  // Stranded escrow (job 987, 26-27 July). A session created but never used
  // strands its deposit for ever: only triggerSessionTimeout frees it, nothing
  // called it, and until it settles the customer's money stays debited. We
  // reclaim rather than credit on a guess, because the state reader cannot tell
  // us whether work was proven and a full refund could over-credit a session
  // the chain will pay a host for.
  it('reclaims a bound session that has sat unsettled past the cutoff', async () => {
    const ledger = await ledgerWithBoundJob(987n);
    const reclaimed: bigint[] = [];
    const alarms: string[] = [];
    const listener = startSettlementListener({
      ledger,
      source: { latestBlock: async () => 10, query: async () => [] },
      cursor: memoryCursor(5).cursor,
      fromBlock: 0,
      onAlarm: (m) => alarms.push(m),
      reclaim: { trigger: async (jobId) => void reclaimed.push(jobId) },
      reclaimAfterMs: 0, // the fixture's hold is "now"; 0 makes every bound job due
      manual: true,
    });

    await listener.tick();

    expect(reclaimed).toEqual([987n]);
    expect(alarms.some((a) => a.includes('reclaimed stranded session 987'))).toBe(true);
    await listener.stop();
  });

  it('leaves a young session alone', async () => {
    // A render in flight must never be cut short by the reclaim.
    const ledger = await ledgerWithBoundJob(988n);
    const reclaimed: bigint[] = [];
    const listener = startSettlementListener({
      ledger,
      source: { latestBlock: async () => 10, query: async () => [] },
      cursor: memoryCursor(5).cursor,
      fromBlock: 0,
      onAlarm: () => {},
      reclaim: { trigger: async (jobId) => void reclaimed.push(jobId) },
      reclaimAfterMs: 60 * 60 * 1000,
      manual: true,
    });

    await listener.tick();

    expect(reclaimed).toEqual([]);
    await listener.stop();
  });

  it('does not credit the customer itself — the settlement event does that', async () => {
    // The whole point: we trigger the timeout and let the contract's own refund
    // figure come back through the tested path. Reclaiming must move no money.
    const ledger = await ledgerWithBoundJob(989n);
    const before = ledger.availableMicro('user-1');
    const listener = startSettlementListener({
      ledger,
      source: { latestBlock: async () => 10, query: async () => [] },
      cursor: memoryCursor(5).cursor,
      fromBlock: 0,
      onAlarm: () => {},
      reclaim: { trigger: async () => {} },
      reclaimAfterMs: 0,
      manual: true,
    });

    await listener.tick();

    expect(ledger.availableMicro('user-1')).toBe(before);
    expect(ledger.boundJobIds()).toEqual([989n]); // still bound until the event lands
    await listener.stop();
  });

  it('alarms but keeps going when a reclaim call fails', async () => {
    const ledger = await ledgerWithBoundJob(990n);
    const alarms: string[] = [];
    const listener = startSettlementListener({
      ledger,
      source: { latestBlock: async () => 10, query: async () => [] },
      cursor: memoryCursor(5).cursor,
      fromBlock: 0,
      onAlarm: (m) => alarms.push(m),
      reclaim: { trigger: async () => { throw new Error('reverted: session still active'); } },
      reclaimAfterMs: 0,
      manual: true,
    });

    await listener.tick();

    expect(alarms.some((a) => a.includes('reclaim failed on job 990'))).toBe(true);
    await listener.stop();
  });

  it('does nothing at all when no reclaim is configured', async () => {
    const ledger = await ledgerWithBoundJob(991n);
    const alarms: string[] = [];
    const listener = startSettlementListener({
      ledger,
      source: { latestBlock: async () => 10, query: async () => [] },
      cursor: memoryCursor(5).cursor,
      fromBlock: 0,
      onAlarm: (m) => alarms.push(m),
      manual: true,
    });
    await listener.tick();
    expect(alarms).toEqual([]);
    await listener.stop();
  });

  it('resumes from the persisted cursor after a restart (replay-safe anyway)', async () => {
    const ledger = await ledgerWithBoundJob(841n);
    const { cursor } = memoryCursor(103);
    const { source, queries } = fakeSource({}, () => 120);
    const listener = startSettlementListener({
      ledger,
      source,
      cursor,
      fromBlock: 100,
      onAlarm: () => {},
      manual: true,
    });
    await listener.tick();
    expect(queries).toEqual([[103, 120]]);
    await listener.stop();
  });

  // The job-960 incident (2026-07-23): a lagging RPC replica served empty logs
  // for a range that contained a real settlement; the cursor advanced and the
  // event was skipped forever. safetyLag + overlapBlocks close both halves.
  it('safetyLag holds back from the head, and the held-back event applies on a later tick', async () => {
    const ledger = await ledgerWithBoundJob(960n);
    let latest = 105;
    // The settlement sits 2 blocks behind the head — inside the lag zone.
    const { source, queries } = fakeSource({ '103': [completed(960n, REFUND, 103)] }, () => latest);
    const { get, cursor } = memoryCursor();
    const listener = startSettlementListener({
      ledger,
      source,
      cursor,
      fromBlock: 100,
      onAlarm: () => {},
      manual: true,
      safetyLag: 5,
    });

    await listener.tick();
    expect(queries).toEqual([[100, 100]]); // scanned only to latest − 5
    expect(get()).toBe(101);
    expect(ledger.refundForJob(960n)).toBeUndefined(); // not seen yet — but not skipped either

    latest = 110;
    await listener.tick(); // head moved; the lag zone now includes block 103
    expect(queries[1]).toEqual([101, 105]);
    expect(ledger.refundForJob(960n)).toBe(REFUND);
    await listener.stop();
  });

  it('overlap re-scans behind the cursor; re-delivery is silent and credits nothing twice', async () => {
    const ledger = await ledgerWithBoundJob(961n);
    const { source, queries } = fakeSource({ '104': [completed(961n, REFUND, 104)] }, () => 110);
    const { get, cursor } = memoryCursor(105); // cursor already PAST the event — the 960 shape
    const alarms: string[] = [];
    const listener = startSettlementListener({
      ledger,
      source,
      cursor,
      fromBlock: 100,
      onAlarm: (m) => alarms.push(m),
      manual: true,
      overlapBlocks: 30,
    });

    await listener.tick(); // scans max(105-30,100)=100 .. 110 → catches the missed event
    expect(queries).toEqual([[100, 110]]);
    expect(ledger.refundForJob(961n)).toBe(REFUND);
    expect(get()).toBe(111);

    await listener.tick(); // overlap re-delivers the same event: idempotent, no alarm
    expect(queries[1]).toEqual([100, 110]);
    expect(ledger.refundForJob(961n)).toBe(REFUND);
    expect(alarms).toEqual([]);
    // cursor never regresses despite scanFrom < cursor
    expect(get()).toBe(111);
    await listener.stop();
  });

  it('cursor never walks backwards when the head stalls inside the lag window', async () => {
    const ledger = await ledgerWithBoundJob(962n);
    const { source } = fakeSource({}, () => 106); // safeLatest = 101 < cursor 105
    const { get, cursor } = memoryCursor(105);
    const listener = startSettlementListener({
      ledger,
      source,
      cursor,
      fromBlock: 100,
      onAlarm: () => {},
      manual: true,
      safetyLag: 5,
      overlapBlocks: 30,
    });
    await listener.tick(); // scans [100,101] via overlap; must not save 102 over 105
    expect(get()).toBe(105);
    await listener.stop();
  });

  // The job-962 incident (2026-07-23, second miss THROUGH the 30-block overlap):
  // log queries can be served by an indexer cluster lagging minutes behind the
  // reported head, so no fixed overlap wins. The state sweep reconciles by
  // eth_call against executed state — log-free, therefore lag-proof.
  it('state sweep recovers a settlement the event path missed entirely, and says so', async () => {
    const ledger = await ledgerWithBoundJob(962n);
    const { source } = fakeSource({}, () => 200); // events: nothing, ever
    const alarms: string[] = [];
    const listener = startSettlementListener({
      ledger,
      source,
      cursor: memoryCursor(190).cursor, // cursor long past the settlement
      fromBlock: 100,
      onAlarm: (m) => alarms.push(m),
      manual: true,
      stateSweep: {
        session: async (jobId) =>
          jobId === 962n ? { ended: true, refundedToUser: REFUND } : undefined,
      },
    });
    await listener.tick();
    expect(ledger.refundForJob(962n)).toBe(REFUND);
    expect(alarms.some((m) => m.includes('state-sweep recovered job 962'))).toBe(true);

    // Recovered job left the bound set — the next tick must not re-settle or re-alarm.
    const before = alarms.length;
    await listener.tick();
    expect(alarms).toHaveLength(before);
    await listener.stop();
  });

  it('state sweep leaves active sessions and unknown reads alone', async () => {
    const ledger = await ledgerWithBoundJob(963n);
    const alarms: string[] = [];
    let answer: { ended: boolean; refundedToUser: bigint } | undefined = {
      ended: false,
      refundedToUser: 0n,
    };
    const listener = startSettlementListener({
      ledger,
      source: fakeSource({}, () => 200).source,
      cursor: memoryCursor(190).cursor,
      fromBlock: 100,
      onAlarm: (m) => alarms.push(m),
      manual: true,
      stateSweep: { session: async () => answer },
    });
    await listener.tick(); // active → untouched
    expect(ledger.refundForJob(963n)).toBeUndefined();
    answer = undefined;
    await listener.tick(); // temporarily unknown → untouched, retried later
    expect(ledger.refundForJob(963n)).toBeUndefined();
    expect(alarms).toEqual([]);
    await listener.stop();
  });

  // The tick-freeze incident (2026-07-23, #3): one hung RPC await stopped ALL
  // future ticks silently — 962 recovered at startup, 965 never touched. The
  // watchdog abandons a stuck tick loudly and the loop lives on; the heartbeat
  // makes prolonged silence provably mean "dead", never "idle".
  it('the watchdog abandons a hung tick with an alarm, and the loop keeps ticking', async () => {
    const ledger = await ledgerWithBoundJob(965n);
    const alarms: string[] = [];
    let hang = true;
    const source = {
      latestBlock: () => (hang ? new Promise<number>(() => {}) : Promise.resolve(200)),
      query: async () => [],
    };
    const listener = startSettlementListener({
      ledger,
      source,
      cursor: memoryCursor(190).cursor,
      fromBlock: 100,
      onAlarm: (m) => alarms.push(m),
      manual: true,
      tickTimeoutMs: 30,
      stateSweep: { session: async () => ({ ended: true, refundedToUser: REFUND }) },
    });

    await listener.tick(); // hangs on latestBlock → watchdog fires
    expect(alarms.some((m) => m.includes('tick watchdog'))).toBe(true);
    expect(ledger.refundForJob(965n)).toBeUndefined();

    hang = false;
    await listener.tick(); // the loop survived; the sweep now recovers the job
    expect(ledger.refundForJob(965n)).toBe(REFUND);
    await listener.stop();
  });

  it('the heartbeat reports tick count, cursor and bound jobs every N ticks', async () => {
    const ledger = await ledgerWithBoundJob(966n);
    const beats: string[] = [];
    const listener = startSettlementListener({
      ledger,
      source: fakeSource({}, () => 200).source,
      cursor: memoryCursor(150).cursor,
      fromBlock: 100,
      onAlarm: () => {},
      manual: true,
      heartbeatEvery: 2,
      onHeartbeat: (line) => beats.push(line),
    });
    await listener.tick();
    await listener.tick();
    await listener.tick();
    await listener.tick();
    expect(beats).toHaveLength(2);
    expect(beats[0]).toContain('tick #2');
    expect(beats[0]).toContain('boundJobs 1');
    expect(beats[1]).toContain('tick #4');
    await listener.stop();
  });

  it('a state-sweep reader failure alarms but does not kill the tick', async () => {
    const ledger = await ledgerWithBoundJob(964n);
    const alarms: string[] = [];
    const { get, cursor } = memoryCursor(190);
    const listener = startSettlementListener({
      ledger,
      source: fakeSource({}, () => 200).source,
      cursor,
      fromBlock: 100,
      onAlarm: (m) => alarms.push(m),
      manual: true,
      stateSweep: {
        session: async () => {
          throw new Error('rpc down');
        },
      },
    });
    await listener.tick();
    expect(alarms.some((m) => m.includes('state-sweep failed on job 964'))).toBe(true);
    expect(get()).toBe(201); // the event path still ran and advanced the cursor
    await listener.stop();
  });

  it('reconciles orphaned holds each tick, then settles the recovered session (M2)', async () => {
    // A hold whose create tx was submitted (txHash known) but never bound — the
    // exact crash-between-create-and-bind orphan.
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('open refused');
    await ledger.recordCreatePending(open.holdId, '0xorphan');

    const alarms: string[] = [];
    const listener = startSettlementListener({
      ledger,
      // Settlement for the recovered job arrives in the same window.
      source: { latestBlock: async () => 10, query: async () => [completed(870n, REFUND, 9)] },
      cursor: { load: async () => 5, save: async () => {} },
      fromBlock: 0,
      onAlarm: (m) => alarms.push(m),
      reconcile: { reader: { read: async () => ({ status: 'created', jobId: 870n }) } },
      manual: true,
    });

    await listener.tick();
    // Reconciliation bound the orphan to job 870, THEN the settlement event for
    // 870 credited the refund to the right user.
    expect(ledger.userForJob(870n)).toBe('user-1');
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n - (DEPOSIT - REFUND));
    expect(alarms.join(' ')).toMatch(/reconciliation: bound 1/);
    await listener.stop();
  });

  it('a source error raises the alarm and the next tick continues', async () => {
    const ledger = await ledgerWithBoundJob(842n);
    const alarms: string[] = [];
    let fail = true;
    const listener = startSettlementListener({
      ledger,
      source: {
        latestBlock: async () => 120,
        query: async () => {
          if (fail) throw new Error('rpc down');
          return [completed(842n, REFUND, 110)];
        },
      },
      cursor: memoryCursor().cursor,
      fromBlock: 100,
      onAlarm: (m) => alarms.push(m),
      manual: true,
    });
    await listener.tick();
    expect(alarms).toHaveLength(1);
    fail = false;
    await listener.tick();
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n - (DEPOSIT - REFUND));
    await listener.stop();
  });
});
