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
