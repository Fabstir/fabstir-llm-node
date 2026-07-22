// FC1.1 — ADVERSARIAL probes against the fiat credits ledger + gatekeeper.
//
// Every probe PASSES when the ledger DEFENDS correctly (i.e. the expected-secure
// behaviour is asserted). A probe that FAILS marks a genuine defect: the secure
// behaviour is asserted and the code does the insecure thing.
//
// Threat model: this ledger is the SOLE authoriser of spends from a platform
// USDC vault; a colluding host converts any unauthorised spend into theft at
// 90%. Any bypass / double-spend / balance inflation / accounting divergence is
// a finding. Amounts are integer USDC micro-units (bigint).
//
// This file MUST NOT modify src/lib/* or the existing test files.
import { afterEach, describe, expect, it } from 'vitest';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { CreditsLedger, JsonlLedgerStore, MemoryLedgerStore } from '../src/lib/ledger';
import {
  gatekeeperConfigFromEnv,
  makeGatekeeper,
  type GatekeeperConfig,
  type LedgerView,
} from '../src/lib/gatekeeper';

const HOST = '0xAbCd000000000000000000000000000000000001';
const OTHER_HOST = '0xdead000000000000000000000000000000000002';

const DEPOSIT = 500_000n;
const SPENT = 200_783n;
const REFUND = DEPOSIT - SPENT;

const CONFIG: GatekeeperConfig = {
  allowedHosts: [HOST],
  maxDepositPerSessionMicro: 2_000_000n,
  maxDailySpendMicro: 10_000_000n,
  maxOpensPerMinute: 3,
};

const gate = makeGatekeeper(CONFIG);

async function freshLedger(opts?: { now?: () => number }) {
  return CreditsLedger.open(new MemoryLedgerStore(), opts);
}

function view(overrides: Partial<LedgerView> = {}): LedgerView {
  return { availableMicro: 5_000_000n, spentInWindowMicro: 0n, opensInWindow: 0, ...overrides };
}

// ---------------------------------------------------------------------------
// Double-spend / concurrency
// ---------------------------------------------------------------------------
describe('double-spend: concurrent opens racing one balance', () => {
  it('N concurrent opens over a one-hold balance: exactly one wins, balance floors at 0', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', DEPOSIT, 'evt_1'); // covers exactly ONE hold
    const results = await Promise.all(
      Array.from({ length: 12 }, () =>
        ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate)
      )
    );
    expect(results.filter((r) => r.ok)).toHaveLength(1);
    expect(ledger.availableMicro('user-1')).toBe(0n);
    // The one winner has a unique holdId (no collision under the race).
    const ids = results.filter((r): r is { ok: true; holdId: string } => r.ok).map((r) => r.holdId);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('two opens each covering half: both win, balance exact (queue is serial, not lossy)', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 2n * DEPOSIT, 'evt_1');
    const [a, b] = await Promise.all([
      ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate),
      ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate),
    ]);
    expect(a.ok && b.ok).toBe(true);
    expect(ledger.availableMicro('user-1')).toBe(0n);
    expect(ledger.outstandingMicro()).toBe(2n * DEPOSIT);
  });
});

describe('interleavings: settle racing release on one bound hold', () => {
  it('settle queued first then release: settle applies, release rejects, balance exact (no double credit)', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 1_000_000n, 'e1');
    const open = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('refused');
    await ledger.bindSession(open.holdId, 5n);
    const [s, r] = await Promise.allSettled([
      ledger.settle(5n, REFUND),
      ledger.releaseHold(open.holdId),
    ]);
    expect(s.status).toBe('fulfilled');
    expect(r.status).toBe('rejected'); // hold is 'bound', release requires 'held'
    expect(ledger.availableMicro('u')).toBe(1_000_000n - SPENT);
  });

  it('release queued first then settle: release rejects, settle applies, balance exact', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 1_000_000n, 'e1');
    const open = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('refused');
    await ledger.bindSession(open.holdId, 6n);
    const [r, s] = await Promise.allSettled([
      ledger.releaseHold(open.holdId),
      ledger.settle(6n, REFUND),
    ]);
    expect(r.status).toBe('rejected');
    expect(s.status).toBe('fulfilled');
    expect(ledger.availableMicro('u')).toBe(1_000_000n - SPENT);
  });
});

// ---------------------------------------------------------------------------
// Balance inflation: amounts, double-settle, state transitions
// ---------------------------------------------------------------------------
describe('balance inflation: non-positive / out-of-range amounts', () => {
  it('purchase rejects zero and negative', async () => {
    const ledger = await freshLedger();
    await expect(ledger.purchase('u', 0n, 'z')).rejects.toThrow();
    await expect(ledger.purchase('u', -1n, 'n')).rejects.toThrow();
    expect(ledger.availableMicro('u')).toBe(0n);
  });

  it('cashout rejects zero and negative and never inflates', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 1_000_000n, 'e1');
    expect(await ledger.cashout('u', 0n)).toEqual({ ok: false, reason: 'INVALID_AMOUNT' });
    expect(await ledger.cashout('u', -1n)).toEqual({ ok: false, reason: 'INVALID_AMOUNT' });
    expect(ledger.availableMicro('u')).toBe(1_000_000n);
  });

  it('settle rejects a negative refund and a refund exceeding the recorded deposit', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 1_000_000n, 'e1');
    const open = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('refused');
    await ledger.bindSession(open.holdId, 7n);
    await expect(ledger.settle(7n, -1n)).rejects.toThrow();
    await expect(ledger.settle(7n, DEPOSIT + 1n)).rejects.toThrow();
    // Nothing credited yet; hold still bound, balance still debited.
    expect(ledger.availableMicro('u')).toBe(1_000_000n - DEPOSIT);
  });

  it('settle with refund == 0 (host took everything) credits nothing back', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 1_000_000n, 'e1');
    const open = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('refused');
    await ledger.bindSession(open.holdId, 8n);
    expect((await ledger.settle(8n, 0n)).applied).toBe(true);
    expect(ledger.availableMicro('u')).toBe(1_000_000n - DEPOSIT);
  });

  it('settle with refund == deposit (full timeout refund) restores exactly', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 1_000_000n, 'e1');
    const open = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('refused');
    await ledger.bindSession(open.holdId, 9n);
    expect((await ledger.settle(9n, DEPOSIT)).applied).toBe(true);
    expect(ledger.availableMicro('u')).toBe(1_000_000n);
  });
});

describe('balance inflation: double-settle and state-machine guards', () => {
  it('settle twice is a strict no-op (single credit only)', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 1_000_000n, 'e1');
    const open = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('refused');
    await ledger.bindSession(open.holdId, 10n);
    expect((await ledger.settle(10n, REFUND)).applied).toBe(true);
    expect((await ledger.settle(10n, REFUND)).applied).toBe(false);
    expect((await ledger.settle(10n, DEPOSIT)).applied).toBe(false); // even with a bigger refund
    expect(ledger.availableMicro('u')).toBe(1_000_000n - SPENT);
  });

  it('concurrent duplicate settles credit exactly once', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 1_000_000n, 'e1');
    const open = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('refused');
    await ledger.bindSession(open.holdId, 11n);
    const outcomes = await Promise.all([
      ledger.settle(11n, REFUND),
      ledger.settle(11n, REFUND),
      ledger.settle(11n, REFUND),
    ]);
    expect(outcomes.filter((o) => o.applied)).toHaveLength(1);
    expect(ledger.availableMicro('u')).toBe(1_000_000n - SPENT);
  });

  it('releaseHold after bind is rejected (bound money can only settle, not double-refund)', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 1_000_000n, 'e1');
    const open = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('refused');
    await ledger.bindSession(open.holdId, 12n);
    await expect(ledger.releaseHold(open.holdId)).rejects.toThrow();
    expect(ledger.availableMicro('u')).toBe(1_000_000n - DEPOSIT);
  });

  it('releaseHold after settle is rejected (no post-settle refund)', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 1_000_000n, 'e1');
    const open = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('refused');
    await ledger.bindSession(open.holdId, 13n);
    await ledger.settle(13n, REFUND);
    await expect(ledger.releaseHold(open.holdId)).rejects.toThrow();
    expect(ledger.availableMicro('u')).toBe(1_000_000n - SPENT);
  });

  it('double releaseHold refunds only once', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 1_000_000n, 'e1');
    const open = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('refused');
    await ledger.releaseHold(open.holdId);
    await expect(ledger.releaseHold(open.holdId)).rejects.toThrow();
    expect(ledger.availableMicro('u')).toBe(1_000_000n);
  });

  it('bindSession twice on one hold is rejected (no rebind)', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 1_000_000n, 'e1');
    const open = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('refused');
    await ledger.bindSession(open.holdId, 14n);
    await expect(ledger.bindSession(open.holdId, 15n)).rejects.toThrow(); // different job
    await expect(ledger.bindSession(open.holdId, 14n)).rejects.toThrow(); // same job
    expect(ledger.userForJob(14n)).toBe('u');
    expect(ledger.userForJob(15n)).toBeUndefined();
  });

  it('bindSession on a released hold is rejected', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 1_000_000n, 'e1');
    const open = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('refused');
    await ledger.releaseHold(open.holdId);
    await expect(ledger.bindSession(open.holdId, 16n)).rejects.toThrow();
    expect(ledger.availableMicro('u')).toBe(1_000_000n);
  });

  it('settle on a hold that was never bound is a no-op (no mapping = not ours)', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 1_000_000n, 'e1');
    const open = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('refused');
    // never bound → no jobId maps to it
    expect((await ledger.settle(999n, DEPOSIT)).applied).toBe(false);
    expect(ledger.availableMicro('u')).toBe(1_000_000n - DEPOSIT);
  });
});

// ---------------------------------------------------------------------------
// DEFECT PROBE: two different holds bound to the SAME jobId
// ---------------------------------------------------------------------------
describe('accounting divergence: two holds bound to the same jobId', () => {
  it('rejects binding a second hold to an already-bound jobId (no silent mapping overwrite)', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('user-1', 1_000_000n, 'evt_a');
    await ledger.purchase('user-2', 1_000_000n, 'evt_b');
    const h0 = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
    const h1 = await ledger.openHold({ userId: 'user-2', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!h0.ok || !h1.ok) throw new Error('open refused');
    await ledger.bindSession(h0.holdId, 5n);

    // SECURE: on-chain jobIds are unique; a second bind to an already-mapped
    // jobId is an accounting divergence (R5), never a silent remap. Under the
    // current code jobToHold.set() overwrites, orphaning h0 forever and
    // redirecting settle(5) to user-2 — a divergence, so this must reject.
    await expect(ledger.bindSession(h1.holdId, 5n)).rejects.toThrow();

    // The original mapping must stand and settle must credit the original owner.
    expect(ledger.userForJob(5n)).toBe('user-1');
  });
});

// ---------------------------------------------------------------------------
// Replay / restart durability
// ---------------------------------------------------------------------------
describe('replay/restart: holdId counter, idempotency sets, cross-user event ids', () => {
  it('holdCounter is reconstructed so a post-restart open never reuses an existing holdId', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'fc1-adv-hc-'));
    const path = join(dir, 'ledger.jsonl');

    const l1 = await CreditsLedger.open(new JsonlLedgerStore(path));
    await l1.purchase('u', 2_000_000n, 'e1');
    const a = await l1.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    const b = await l1.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!a.ok || !b.ok) throw new Error('open refused');
    expect(a.holdId).toBe('h0');
    expect(b.holdId).toBe('h1');

    const l2 = await CreditsLedger.open(new JsonlLedgerStore(path));
    const c = await l2.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!c.ok) throw new Error('open refused after restart');
    // Must be a fresh id, not colliding with h0/h1 (which would corrupt a hold).
    expect(c.holdId).toBe('h2');
    expect(l2.availableMicro('u')).toBe(2_000_000n - 3n * DEPOSIT);
    expect(l2.outstandingMicro()).toBe(2_000_000n);
  });

  it('the SAME stripe eventId for a DIFFERENT user after restart is still a no-op (idempotency is per-event, not per-user)', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'fc1-adv-evt-'));
    const path = join(dir, 'ledger.jsonl');

    const l1 = await CreditsLedger.open(new JsonlLedgerStore(path));
    await l1.purchase('user-1', 1_000_000n, 'evt_shared');

    const l2 = await CreditsLedger.open(new JsonlLedgerStore(path));
    const attempt = await l2.purchase('user-2', 1_000_000n, 'evt_shared');
    expect(attempt.applied).toBe(false);
    expect(l2.availableMicro('user-2')).toBe(0n);
    expect(l2.availableMicro('user-1')).toBe(1_000_000n);
  });

  it('a settled job stays settled across restart (replayed settle never double-credits)', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'fc1-adv-settle-'));
    const path = join(dir, 'ledger.jsonl');

    const l1 = await CreditsLedger.open(new JsonlLedgerStore(path));
    await l1.purchase('u', 1_000_000n, 'e1');
    const open = await l1.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('refused');
    await l1.bindSession(open.holdId, 42n);
    await l1.settle(42n, REFUND);
    const before = l1.availableMicro('u');

    const l2 = await CreditsLedger.open(new JsonlLedgerStore(path));
    expect(l2.availableMicro('u')).toBe(before);
    expect((await l2.settle(42n, REFUND)).applied).toBe(false);
    expect((await l2.settle(42n, DEPOSIT)).applied).toBe(false);
    expect(l2.availableMicro('u')).toBe(before);
  });
});

// ---------------------------------------------------------------------------
// Gatekeeper evasion
// ---------------------------------------------------------------------------
describe('gatekeeper evasion: host string tricks fail closed', () => {
  it('trailing whitespace in the request host does not match the allow-list', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 1_000_000n, 'e1');
    const open = await ledger.openHold({ userId: 'u', host: `${HOST} `, depositMicro: DEPOSIT }, gate);
    expect(open).toEqual({ ok: false, reason: 'HOST_NOT_ALLOWED' });
    expect(ledger.availableMicro('u')).toBe(1_000_000n);
  });

  it('a zero-width character in the request host does not match the allow-list', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 1_000_000n, 'e1');
    const open = await ledger.openHold({ userId: 'u', host: `​${HOST}`, depositMicro: DEPOSIT }, gate);
    expect(open).toEqual({ ok: false, reason: 'HOST_NOT_ALLOWED' });
  });

  it('leading/trailing case+space variant of an off-list host stays refused', async () => {
    expect(gate(view(), { host: ` ${OTHER_HOST.toUpperCase()} `, depositMicro: 500_000n })).toEqual({
      allow: false,
      reason: 'HOST_NOT_ALLOWED',
    });
  });
});

describe('gatekeeper evasion: cap and window boundaries', () => {
  it('a deposit exactly at the per-session cap is allowed; cap+1 is refused', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 5_000_000n, 'e1');
    const atCap = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: 2_000_000n }, gate);
    expect(atCap.ok).toBe(true);
    const overCap = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: 2_000_001n }, gate);
    expect(overCap).toEqual({ ok: false, reason: 'DEPOSIT_OVER_CAP' });
  });

  it('daily cap counts the requested deposit: spent+deposit == cap allowed, +1 over cap refused', () => {
    expect(gate(view({ spentInWindowMicro: 9_500_000n }), { host: HOST, depositMicro: 500_000n })).toEqual({
      allow: true,
    });
    expect(gate(view({ spentInWindowMicro: 9_500_001n }), { host: HOST, depositMicro: 500_000n })).toEqual({
      allow: false,
      reason: 'DAILY_CAP_EXCEEDED',
    });
  });

  it('rate limit triggers exactly at the configured count (>=), not one late', () => {
    const g = makeGatekeeper({ ...CONFIG, maxOpensPerMinute: 3 });
    expect(g(view({ opensInWindow: 2 }), { host: HOST, depositMicro: 500_000n })).toEqual({ allow: true });
    expect(g(view({ opensInWindow: 3 }), { host: HOST, depositMicro: 500_000n })).toEqual({
      allow: false,
      reason: 'RATE_LIMITED',
    });
  });

  it('a hold exactly 24h old has dropped out of the rolling daily window (boundary is exclusive)', async () => {
    let nowMs = 1_700_000_000_000;
    const ledger = await freshLedger({ now: () => nowMs });
    const tight = makeGatekeeper({ ...CONFIG, maxDailySpendMicro: DEPOSIT, maxOpensPerMinute: 100 });
    await ledger.purchase('u', 10_000_000n, 'e1');
    expect((await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, tight)).ok).toBe(true);
    // Exactly 24h later the first hold no longer counts, so a second is allowed.
    nowMs += 24 * 60 * 60 * 1000;
    expect((await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, tight)).ok).toBe(true);
  });

  it('an empty allow-list refuses every open regardless of a healthy balance', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('u', 10_000_000n, 'e1');
    const closed = makeGatekeeper({ ...CONFIG, allowedHosts: [] });
    const open = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, closed);
    expect(open).toEqual({ ok: false, reason: 'HOST_NOT_ALLOWED' });
    expect(ledger.availableMicro('u')).toBe(10_000_000n);
  });
});

// ---------------------------------------------------------------------------
// Config parsing from env
// ---------------------------------------------------------------------------
describe('gatekeeperConfigFromEnv: hostile env values', () => {
  afterEach(() => {
    delete process.env.FIAT_ALLOWED_HOSTS;
    delete process.env.FIAT_MAX_SESSION_DEPOSIT_MICRO;
    delete process.env.FIAT_MAX_DAILY_SPEND_MICRO;
    delete process.env.FIAT_MAX_OPENS_PER_MINUTE;
  });

  it('drops empty entries from a messy comma list (no accidental "" host)', () => {
    process.env.FIAT_ALLOWED_HOSTS = `,, ${HOST} ,,`;
    const config = gatekeeperConfigFromEnv();
    expect(config.allowedHosts).toEqual([HOST]);
    // An empty-string host must never be allowed.
    const g = makeGatekeeper(config);
    expect(g(view(), { host: '', depositMicro: 500_000n })).toEqual({ allow: false, reason: 'HOST_NOT_ALLOWED' });
  });

  it('a whitespace-only allow-list yields an empty (refuse-all) list', () => {
    process.env.FIAT_ALLOWED_HOSTS = '   ';
    expect(gatekeeperConfigFromEnv().allowedHosts).toEqual([]);
  });

  it('a negative session-deposit cap fails closed (refuses all positive deposits)', () => {
    process.env.FIAT_ALLOWED_HOSTS = HOST;
    process.env.FIAT_MAX_SESSION_DEPOSIT_MICRO = '-1';
    const g = makeGatekeeper(gatekeeperConfigFromEnv());
    expect(g(view(), { host: HOST, depositMicro: 500_000n })).toEqual({
      allow: false,
      reason: 'DEPOSIT_OVER_CAP',
    });
  });

  it('a zero session-deposit cap fails closed', () => {
    process.env.FIAT_ALLOWED_HOSTS = HOST;
    process.env.FIAT_MAX_SESSION_DEPOSIT_MICRO = '0';
    const g = makeGatekeeper(gatekeeperConfigFromEnv());
    expect(g(view(), { host: HOST, depositMicro: 1n }).allow).toBe(false);
  });

  it('a zero opens-per-minute cap fails closed (refuses even the first open)', () => {
    process.env.FIAT_ALLOWED_HOSTS = HOST;
    process.env.FIAT_MAX_OPENS_PER_MINUTE = '0';
    const g = makeGatekeeper(gatekeeperConfigFromEnv());
    expect(g(view({ opensInWindow: 0 }), { host: HOST, depositMicro: 500_000n })).toEqual({
      allow: false,
      reason: 'RATE_LIMITED',
    });
  });

  it('a negative opens-per-minute is rejected at config time (not silently coerced)', () => {
    process.env.FIAT_MAX_OPENS_PER_MINUTE = '-1';
    expect(() => gatekeeperConfigFromEnv()).toThrow(/FIAT_MAX_OPENS_PER_MINUTE/);
  });

  it('a non-integer bigint cap is rejected at config time', () => {
    process.env.FIAT_MAX_SESSION_DEPOSIT_MICRO = '2.5';
    expect(() => gatekeeperConfigFromEnv()).toThrow(/FIAT_MAX_SESSION_DEPOSIT_MICRO/);
  });

  it('a non-integer opens value is rejected at config time', () => {
    process.env.FIAT_MAX_OPENS_PER_MINUTE = '3.5';
    expect(() => gatekeeperConfigFromEnv()).toThrow(/FIAT_MAX_OPENS_PER_MINUTE/);
  });
});

// ---------------------------------------------------------------------------
// Solvency invariant under a full lifecycle
// ---------------------------------------------------------------------------
describe('solvency: outstanding never inflates over a full multi-user lifecycle', () => {
  it('purchase + hold + settle + cashout keeps outstanding == sum of what the vault owes', async () => {
    const ledger = await freshLedger();
    await ledger.purchase('a', 1_000_000n, 'ea');
    await ledger.purchase('b', 1_000_000n, 'eb');
    const ha = await ledger.openHold({ userId: 'a', host: HOST, depositMicro: DEPOSIT }, gate);
    const hb = await ledger.openHold({ userId: 'b', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!ha.ok || !hb.ok) throw new Error('refused');
    // both held: money moved from available to held, none left the ledger.
    expect(ledger.outstandingMicro()).toBe(2_000_000n);

    await ledger.bindSession(ha.holdId, 100n);
    await ledger.settle(100n, REFUND); // SPENT leaves to host/treasury
    await ledger.releaseHold(hb.holdId); // b's create tx failed, full reversal
    await ledger.cashout('b', 200_000n); // b cashes out via Stripe

    expect(ledger.outstandingMicro()).toBe(2_000_000n - SPENT - 200_000n);
    expect(ledger.availableMicro('a')).toBe(1_000_000n - SPENT);
    expect(ledger.availableMicro('b')).toBe(800_000n);
  });
});
