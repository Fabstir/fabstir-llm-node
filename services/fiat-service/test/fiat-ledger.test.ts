// FC1.1 — the credits ledger: the fiat user's money record (IMPLEMENTATION-
// FIAT-CREDITS-VAULT.md). All amounts are integer USDC micro-units (bigint).
// The ledger is the ONLY authoriser of a vault spend: openHold runs the
// gatekeeper and places the hold atomically, so concurrent opens cannot
// double-spend one balance.
import { describe, expect, it } from 'vitest';
import { appendFileSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { CreditsLedger, JsonlLedgerStore, MemoryLedgerStore } from '../src/lib/ledger';
import { makeGatekeeper, type GatekeeperConfig } from '../src/lib/gatekeeper';

const HOST = '0xAbCd000000000000000000000000000000000001';
const OTHER_HOST = '0xdead000000000000000000000000000000000002';

// Mirrors live session economics: 0.5 USDC deposit, 720p 10s settles ~200,783.
const DEPOSIT = 500_000n;
const SPENT = 200_783n;
const REFUND = DEPOSIT - SPENT;

const CONFIG: GatekeeperConfig = {
  allowedHosts: [HOST],
  maxDepositPerSessionMicro: 2_000_000n,
  maxDailySpendMicro: 10_000_000n,
  maxOpensPerMinute: 3,
};

async function freshLedger(opts?: { now?: () => number }) {
  return CreditsLedger.open(new MemoryLedgerStore(), opts);
}

const gate = makeGatekeeper(CONFIG);

describe('purchases (Stripe webhook is the only credit path)', () => {
  it('credits the balance once per Stripe event id (replay is a no-op)', async () => {
    const ledger = await freshLedger();
    const first = await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const replay = await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    expect(first.applied).toBe(true);
    expect(replay.applied).toBe(false);
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n);
  });

  it('rejects a non-positive purchase amount', async () => {
    const ledger = await freshLedger();
    await expect(ledger.purchase('user-1', 0n, 'evt_z')).rejects.toThrow();
    await expect(ledger.purchase('user-1', -5n, 'evt_n')).rejects.toThrow();
  });

  it('keeps balances per user', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    await ledger.purchase('user-2', 700_000n, 'evt_2');
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n);
    expect(ledger.availableMicro('user-2')).toBe(700_000n);
    expect(ledger.availableMicro('nobody')).toBe(0n);
  });
});

describe('hold -> settle reconciles to the penny', () => {
  it('debits the deposit on hold and credits back exactly the userRefund on settle', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');

    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
    expect(open.ok).toBe(true);
    if (!open.ok) return;
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n - DEPOSIT);

    await ledger.bindSession(open.holdId, 818n);
    // SessionCompleted(jobId, totalTokensUsed, hostEarnings, userRefund) — keyed on userRefund.
    const settle = await ledger.settle(818n, REFUND);
    expect(settle.applied).toBe(true);
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n - SPENT);
  });

  it('a zombie/timeout full refund reverses the hold exactly', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('open refused');
    await ledger.bindSession(open.holdId, 819n);
    await ledger.settle(819n, DEPOSIT); // SessionTimedOut userRefund == full deposit
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n);
  });

  it('a replayed settlement event is a no-op', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('open refused');
    await ledger.bindSession(open.holdId, 820n);
    await ledger.settle(820n, REFUND);
    const replay = await ledger.settle(820n, REFUND);
    expect(replay.applied).toBe(false);
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n - SPENT);
  });

  it('settling an unknown jobId is a no-op (not our session)', async () => {
    const ledger = await freshLedger();
    const settle = await ledger.settle(999_999n, 123n);
    expect(settle.applied).toBe(false);
  });

  it('a refund larger than the recorded deposit is rejected (divergence alarm, not silent credit)', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('open refused');
    await ledger.bindSession(open.holdId, 821n);
    await expect(ledger.settle(821n, DEPOSIT + 1n)).rejects.toThrow();
  });

  it('releaseHold (create tx failed before any session existed) reverses in full', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('open refused');
    await ledger.releaseHold(open.holdId);
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n);
  });

  it('maps jobId back to the owning user for the settlement listener', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('open refused');
    await ledger.bindSession(open.holdId, 822n);
    expect(ledger.userForJob(822n)).toBe('user-1');
    expect(ledger.userForJob(4n)).toBeUndefined();
  });
});

describe('the gatekeeper refuses before any money moves', () => {
  it('refuses an over-balance open and changes nothing', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 400_000n, 'evt_1');
    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
    expect(open).toEqual({ ok: false, reason: 'INSUFFICIENT_BALANCE' });
    expect(ledger.availableMicro('user-1')).toBe(400_000n);
  });

  it('refuses an off-allow-list host (case-insensitive compare)', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const open = await ledger.openHold({ userId: 'user-1', host: OTHER_HOST, depositMicro: DEPOSIT }, gate);
    expect(open).toEqual({ ok: false, reason: 'HOST_NOT_ALLOWED' });

    const lowercased = await ledger.openHold(
      { userId: 'user-1', host: HOST.toLowerCase(), depositMicro: DEPOSIT },
      gate
    );
    expect(lowercased.ok).toBe(true);
  });

  it('refuses a deposit over the per-session cap', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 5_000_000n, 'evt_1');
    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: 2_000_001n }, gate);
    expect(open).toEqual({ ok: false, reason: 'DEPOSIT_OVER_CAP' });
  });

  it('refuses a non-positive deposit', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: 0n }, gate);
    expect(open).toEqual({ ok: false, reason: 'INVALID_DEPOSIT' });
  });

  it('enforces the rolling daily velocity cap', async () => {
    let nowMs = 1_700_000_000_000;
    const ledger = await freshLedger({ now: () => nowMs });
    const config: GatekeeperConfig = { ...CONFIG, maxDailySpendMicro: 1_000_000n, maxOpensPerMinute: 100 };
    const tightGate = makeGatekeeper(config);
    await ledger.purchase('user-1', 10_000_000n, 'evt_1');

    const first = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: 600_000n }, tightGate);
    expect(first.ok).toBe(true);
    nowMs += 60_000;
    const second = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: 600_000n }, tightGate);
    expect(second).toEqual({ ok: false, reason: 'DAILY_CAP_EXCEEDED' });

    nowMs += 25 * 60 * 60 * 1000; // window rolls past the first hold
    const third = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: 600_000n }, tightGate);
    expect(third.ok).toBe(true);
  });

  it('rate-limits opens per minute per user', async () => {
    let nowMs = 1_700_000_000_000;
    const ledger = await freshLedger({ now: () => nowMs });
    const config: GatekeeperConfig = { ...CONFIG, maxOpensPerMinute: 2 };
    const tightGate = makeGatekeeper(config);
    await ledger.purchase('user-1', 10_000_000n, 'evt_1');

    expect((await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, tightGate)).ok).toBe(true);
    nowMs += 1000;
    expect((await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, tightGate)).ok).toBe(true);
    nowMs += 1000;
    const third = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, tightGate);
    expect(third).toEqual({ ok: false, reason: 'RATE_LIMITED' });

    nowMs += 61_000;
    const later = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, tightGate);
    expect(later.ok).toBe(true);
  });

  it('concurrent opens on one balance cannot double-spend', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', DEPOSIT, 'evt_1'); // covers exactly ONE hold
    const [a, b] = await Promise.all([
      ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate),
      ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate),
    ]);
    expect([a.ok, b.ok].filter(Boolean)).toHaveLength(1);
    expect(ledger.availableMicro('user-1')).toBe(0n);
  });
});

describe('cash-out (Stripe refund, never USDC)', () => {
  it('debits the balance and refuses an over-balance cash-out', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const ok = await ledger.cashout('user-1', 300_000n);
    expect(ok).toEqual({ ok: true });
    expect(ledger.availableMicro('user-1')).toBe(700_000n);

    const over = await ledger.cashout('user-1', 700_001n);
    expect(over).toEqual({ ok: false, reason: 'INSUFFICIENT_BALANCE' });
    expect(ledger.availableMicro('user-1')).toBe(700_000n);
  });

  it('refuses a non-positive cash-out', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    expect(await ledger.cashout('user-1', 0n)).toEqual({ ok: false, reason: 'INVALID_AMOUNT' });
  });
});

describe('solvency: outstanding ledger money', () => {
  it('counts available balances plus active holds, across users', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    await ledger.purchase('user-2', 700_000n, 'evt_2');
    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('open refused');

    // 1.0M + 0.7M: a hold moves money from available to held, not out of the ledger.
    expect(ledger.outstandingMicro()).toBe(1_700_000n);

    await ledger.bindSession(open.holdId, 830n);
    await ledger.settle(830n, REFUND);
    // The spent part left the ledger to the host/treasury via the vault.
    expect(ledger.outstandingMicro()).toBe(1_700_000n - SPENT);
  });
});

describe('durability (JSONL journal, D1)', () => {
  it('tolerates a torn trailing line but rejects earlier corruption (L4)', async () => {
    // Simulate a crash mid-append: a valid journal plus a truncated final line.
    const dir = mkdtempSync(join(tmpdir(), 'fc1-torn-'));
    const path = join(dir, 'ledger.jsonl');
    const ledger = await CreditsLedger.open(new JsonlLedgerStore(path));
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    appendFileSync(path, '{"t":"purchase","userId":"user-1","amount":"5000'); // torn

    const reopened = await CreditsLedger.open(new JsonlLedgerStore(path));
    expect(reopened.availableMicro('user-1')).toBe(1_000_000n); // torn line dropped

    // Corruption that is NOT the last line is real damage and must throw.
    writeFileSync(path, 'not json\n{"t":"purchase","userId":"user-1","amount":"1000000","eventId":"e","at":1}\n');
    await expect(CreditsLedger.open(new JsonlLedgerStore(path))).rejects.toThrow(/corrupt/);
  });

  it('enumerates unbound holds for R5 reconciliation (M2)', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('open refused');
    // Not yet bound (simulating a crash before bindSession).
    expect(ledger.unboundHolds()).toEqual([
      { holdId: open.holdId, userId: 'user-1', host: HOST, amountMicro: DEPOSIT, atMs: expect.any(Number) },
    ]);
    await ledger.bindSession(open.holdId, 900n);
    expect(ledger.unboundHolds()).toEqual([]); // bound -> no longer an orphan candidate
  });

  it('records a create-pending tx hash and surfaces it for deterministic reconciliation (M2)', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('open refused');

    // No pending tx yet.
    expect(ledger.pendingCreates()).toEqual([]);

    // The create tx was submitted (hash known) but a crash struck before bind.
    await ledger.recordCreatePending(open.holdId, '0xdeadbeef');
    expect(ledger.pendingCreates()).toEqual([
      { holdId: open.holdId, userId: 'user-1', host: HOST, amountMicro: DEPOSIT, txHash: '0xdeadbeef', atMs: expect.any(Number) },
    ]);

    // Binding (the reconciliation outcome) clears the pending marker.
    await ledger.bindSession(open.holdId, 901n);
    expect(ledger.pendingCreates()).toEqual([]);
    expect(ledger.userForJob(901n)).toBe('user-1');
  });

  it('releasing a pending hold (create tx reverted) clears it and restores the balance', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('open refused');
    await ledger.recordCreatePending(open.holdId, '0xreverted');
    await ledger.releaseHold(open.holdId);
    expect(ledger.pendingCreates()).toEqual([]);
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n);
  });

  it('replays a create-pending marker across a restart (survives the crash it guards against)', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'fc1-pending-'));
    const path = join(dir, 'ledger.jsonl');
    const ledger = await CreditsLedger.open(new JsonlLedgerStore(path));
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('open refused');
    await ledger.recordCreatePending(open.holdId, '0xacecafe');

    const reopened = await CreditsLedger.open(new JsonlLedgerStore(path));
    expect(reopened.pendingCreates()).toEqual([
      { holdId: open.holdId, userId: 'user-1', host: HOST, amountMicro: DEPOSIT, txHash: '0xacecafe', atMs: expect.any(Number) },
    ]);
  });

  it('replays the journal to identical balances after a restart', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'fc1-ledger-'));
    const path = join(dir, 'ledger.jsonl');

    const ledger = await CreditsLedger.open(new JsonlLedgerStore(path));
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('open refused');
    await ledger.bindSession(open.holdId, 840n);
    await ledger.settle(840n, REFUND);
    await ledger.cashout('user-1', 100_000n);

    const reopened = await CreditsLedger.open(new JsonlLedgerStore(path));
    expect(reopened.availableMicro('user-1')).toBe(ledger.availableMicro('user-1'));
    expect(reopened.outstandingMicro()).toBe(ledger.outstandingMicro());
    expect(reopened.userForJob(840n)).toBe('user-1');
    // A replayed settlement stays a no-op across the restart.
    expect((await reopened.settle(840n, REFUND)).applied).toBe(false);
    // And a replayed Stripe event stays a no-op too.
    expect((await reopened.purchase('user-1', 1_000_000n, 'evt_1')).applied).toBe(false);
  });
});
