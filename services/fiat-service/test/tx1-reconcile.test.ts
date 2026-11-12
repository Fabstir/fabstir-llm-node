// TX1 §8 — reconciliation of Stripe against the journal: charges (gross, tax,
// net per payment intent), refunds in Stripe with no ledger entry and the
// reverse, disputes, and the total of balances below zero. No network: the
// Stripe side is a scripted source.
import { afterEach, describe, expect, it } from 'vitest';
import { CreditsLedger, MemoryLedgerStore, cashoutLegKey } from '../src/lib/ledger';
import {
  reconcileStripe,
  setStripeReconcileForTest,
  type StripeDisputeRow,
  type StripeReconcileSource,
  type StripeRefundRow,
  type StripeSessionRow,
} from '../src/lib/fiat-reconcile';
import { setFiatDepsForTest } from '../src/lib/fiat-session-service';
import { POST as reconcileRoute } from '../app/v1/fiat/stripe/reconcile/route';

const DAY_MS = 86_400_000;
const NOW = 1_794_000_000_000;
const HOUR_AGO_S = Math.floor((NOW - 3_600_000) / 1000);

function source(rows: { sessions?: StripeSessionRow[]; refunds?: StripeRefundRow[]; disputes?: StripeDisputeRow[] }): StripeReconcileSource {
  return {
    listPaidSessions: async () => rows.sessions ?? [],
    listRefunds: async () => rows.refunds ?? [],
    listDisputes: async () => rows.disputes ?? [],
  };
}

const session = (pi: string, grossCents: number, taxCents: number, over: Partial<StripeSessionRow> = {}): StripeSessionRow => ({
  sessionId: `cs_${pi}`,
  paymentIntentId: pi,
  userId: 'u',
  grossCents,
  taxCents,
  currency: 'usd',
  createdS: HOUR_AGO_S,
  ...over,
});

async function ledgerWith() {
  let clock = NOW - 3_600_000;
  const ledger = await CreditsLedger.open(new MemoryLedgerStore(), { now: () => clock });
  await ledger.purchase('u', 10_000_000n, 'evt_1', { paymentIntentId: 'pi_1', grossCents: 1200, taxCents: 200 });
  const setClock = (ms: number) => {
    clock = ms;
  };
  return { ledger, setClock };
}

async function run(ledger: CreditsLedger, rows: Parameters<typeof source>[0]) {
  const alarms: string[] = [];
  const report = await reconcileStripe({
    ledger,
    source: source(rows),
    onAlarm: (m) => alarms.push(m),
    now: () => NOW,
    lookbackMs: 100 * DAY_MS,
  });
  return { alarms, report };
}

describe('TX1 §8 reconcileStripe', () => {
  it('is silent when Stripe and the journal agree', async () => {
    const { ledger } = await ledgerWith();
    const { alarms, report } = await run(ledger, { sessions: [session('pi_1', 1200, 200)] });
    expect(alarms).toEqual([]);
    expect(report).toMatchObject({ sessions: 1, findings: [] });
  });

  it('a paid session of ours that was never credited or refused is reported', async () => {
    const { ledger } = await ledgerWith();
    const { alarms } = await run(ledger, { sessions: [session('pi_1', 1200, 200), session('pi_missing', 600, 100)] });
    expect(alarms).toHaveLength(1);
    expect(alarms[0]).toMatch(/pi_missing/);
    expect(alarms[0]).toMatch(/not in the ledger/);
  });

  it("another product's session (no fc1UserId) and a session a few minutes old are left alone", async () => {
    const { ledger } = await ledgerWith();
    const { alarms } = await run(ledger, {
      sessions: [
        session('pi_1', 1200, 200),
        session('pi_v2', 900, 150, { userId: undefined }),
        session('pi_fresh', 600, 100, { createdS: Math.floor(NOW / 1000) - 60 }),
      ],
    });
    expect(alarms).toEqual([]);
  });

  it('gross, tax or net that differ from the journal are reported per payment intent', async () => {
    const { ledger } = await ledgerWith();
    const { alarms } = await run(ledger, { sessions: [session('pi_1', 1250, 250)] });
    expect(alarms).toHaveLength(1);
    expect(alarms[0]).toMatch(/pi_1/);
    expect(alarms[0]).toMatch(/gross 1250.*1200|1200.*1250/);
  });

  it('a refund in Stripe with no ledger entry is reported, and so is the reverse', async () => {
    const { ledger, setClock } = await ledgerWith();
    // In Stripe, not in the ledger (its webhook never arrived).
    const a = await run(ledger, {
      sessions: [session('pi_1', 1200, 200)],
      refunds: [{ refundId: 're_x', paymentIntentId: 'pi_1', amountCents: 300, status: 'succeeded' }],
    });
    expect(a.alarms).toHaveLength(1);
    expect(a.alarms[0]).toMatch(/pi_1.*Stripe.*300.*ledger.*0/);

    // In the ledger (a confirmed leg), not in Stripe.
    setClock(NOW - 2 * 3_600_000);
    await ledger.planCashout('u', 2_000_000n, { requestId: 'r1', windowMs: 90 * DAY_MS });
    await ledger.confirmCashoutLeg(cashoutLegKey('r1', 'pi_1'), 're_gone');
    const b = await run(ledger, { sessions: [session('pi_1', 1200, 200)], refunds: [] });
    expect(b.alarms).toHaveLength(1);
    expect(b.alarms[0]).toMatch(/pi_1.*Stripe.*0.*ledger.*240/);

    // Agreeing figures are silent; a failed refund in Stripe does not count.
    const c = await run(ledger, {
      sessions: [session('pi_1', 1200, 200)],
      refunds: [
        { refundId: 're_gone', paymentIntentId: 'pi_1', amountCents: 240, status: 'succeeded', key: cashoutLegKey('r1', 'pi_1') },
        { refundId: 're_f', paymentIntentId: 'pi_1', amountCents: 100, status: 'failed' },
      ],
    });
    expect(c.alarms).toEqual([]);
  });

  it('a refund leg still pending after the grace period is reported with what Stripe shows for its key', async () => {
    const { ledger, setClock } = await ledgerWith();
    setClock(NOW - 2 * 3_600_000);
    await ledger.planCashout('u', 2_000_000n, { requestId: 'r9', windowMs: 90 * DAY_MS });
    const key = cashoutLegKey('r9', 'pi_1');
    const none = await run(ledger, { sessions: [session('pi_1', 1200, 200)] });
    expect(none.alarms.some((m) => m.includes(key) && /no refund/i.test(m))).toBe(true);
    const landed = await run(ledger, {
      sessions: [session('pi_1', 1200, 200)],
      refunds: [{ refundId: 're_l', paymentIntentId: 'pi_1', amountCents: 240, status: 'succeeded', key }],
    });
    expect(landed.alarms).toHaveLength(1);
    expect(landed.alarms[0]).toMatch(/re_l/);
  });

  it('a refused purchase must be refunded in full', async () => {
    const { ledger } = await ledgerWith();
    await ledger.recordRefusedPurchase({ userId: 'x', eventId: 'evt_r', paymentIntentId: 'pi_r', grossCents: 500, taxCents: 0, reason: 'country' });
    const missing = await run(ledger, { sessions: [session('pi_1', 1200, 200), session('pi_r', 500, 0, { userId: 'x' })] });
    expect(missing.alarms).toHaveLength(1);
    expect(missing.alarms[0]).toMatch(/pi_r.*refused.*500/);
    const done = await run(ledger, {
      sessions: [session('pi_1', 1200, 200), session('pi_r', 500, 0, { userId: 'x' })],
      refunds: [{ refundId: 're_g', paymentIntentId: 'pi_r', amountCents: 500, status: 'succeeded', origin: 'gate' }],
    });
    expect(done.alarms).toEqual([]);
  });

  it('disputes: one Stripe has and the ledger lacks, and a state that differs, are reported', async () => {
    const { ledger } = await ledgerWith();
    const missing = await run(ledger, {
      sessions: [session('pi_1', 1200, 200)],
      disputes: [{ disputeId: 'dp_1', paymentIntentId: 'pi_1', amountCents: 1200, status: 'needs_response' }],
    });
    expect(missing.alarms).toHaveLength(1);
    expect(missing.alarms[0]).toMatch(/dp_1.*not in the ledger/);

    await ledger.disputeOpened({ disputeId: 'dp_1', paymentIntentId: 'pi_1', amountCents: 1200, eventId: 'evt_dp' });
    const open = await run(ledger, {
      sessions: [session('pi_1', 1200, 200)],
      disputes: [{ disputeId: 'dp_1', paymentIntentId: 'pi_1', amountCents: 1200, status: 'under_review' }],
    });
    expect(open.alarms).toEqual([]);

    const lostAtStripe = await run(ledger, {
      sessions: [session('pi_1', 1200, 200)],
      disputes: [{ disputeId: 'dp_1', paymentIntentId: 'pi_1', amountCents: 1200, status: 'lost' }],
    });
    expect(lostAtStripe.alarms).toHaveLength(1);
    expect(lostAtStripe.alarms[0]).toMatch(/dp_1.*lost.*open/);

    // A dispute on another product's charge is not ours to report.
    const other = await run(ledger, {
      sessions: [session('pi_1', 1200, 200)],
      disputes: [
        { disputeId: 'dp_1', paymentIntentId: 'pi_1', amountCents: 1200, status: 'needs_response' },
        { disputeId: 'dp_v2', paymentIntentId: 'pi_v2', amountCents: 900, status: 'needs_response' },
      ],
    });
    expect(other.alarms).toEqual([]);
  });

  it('a dispute Stripe closed as prevented or charge_refunded matches the ledger\'s "debit stands": no false mismatch', async () => {
    for (const status of ['prevented', 'charge_refunded']) {
      const { ledger } = await ledgerWith();
      await ledger.disputeOpened({ disputeId: 'dp_1', paymentIntentId: 'pi_1', amountCents: 1200, eventId: 'evt_dp' });
      await ledger.disputeClosed({ disputeId: 'dp_1', outcome: 'lost', eventId: 'evt_dc' }); // what the webhook journals for these
      const { alarms } = await run(ledger, {
        sessions: [session('pi_1', 1200, 200)],
        disputes: [{ disputeId: 'dp_1', paymentIntentId: 'pi_1', amountCents: 1200, status }],
      });
      expect(alarms, status).toEqual([]);
    }
    // And a real difference is still reported: Stripe says won, the ledger kept the debit.
    const { ledger } = await ledgerWith();
    await ledger.disputeOpened({ disputeId: 'dp_1', paymentIntentId: 'pi_1', amountCents: 1200, eventId: 'evt_dp' });
    await ledger.disputeClosed({ disputeId: 'dp_1', outcome: 'lost', eventId: 'evt_dc' });
    const won = await run(ledger, {
      sessions: [session('pi_1', 1200, 200)],
      disputes: [{ disputeId: 'dp_1', paymentIntentId: 'pi_1', amountCents: 1200, status: 'won' }],
    });
    expect(won.alarms).toHaveLength(1);
  });

  it('reports the total of balances below zero', async () => {
    const { ledger } = await ledgerWith();
    await ledger.cashout('u', 10_000_000n); // spent
    await ledger.chargeRefunded('pi_1', 1200, 'evt_dash');
    const { alarms, report } = await run(ledger, {
      sessions: [session('pi_1', 1200, 200)],
      refunds: [{ refundId: 're_d', paymentIntentId: 'pi_1', amountCents: 1200, status: 'succeeded' }],
    });
    expect(report.receivableMicro).toBe(10_000_000n);
    expect(alarms).toHaveLength(1);
    expect(alarms[0]).toMatch(/below zero.*10000000/);
  });
});

describe('POST /v1/fiat/stripe/reconcile (the daily cron kick)', () => {
  afterEach(() => {
    delete process.env.FIAT_ADMIN_TOKEN;
    setFiatDepsForTest(undefined);
    setStripeReconcileForTest(undefined);
  });

  const post = (token?: string) =>
    reconcileRoute(
      new Request('http://fiat/v1/fiat/stripe/reconcile', {
        method: 'POST',
        headers: token ? { authorization: `Bearer ${token}` } : {},
      })
    );

  it('is disabled without FIAT_ADMIN_TOKEN and refuses a wrong token', async () => {
    expect((await post('x')).status).toBe(503);
    process.env.FIAT_ADMIN_TOKEN = 'admin';
    expect((await post('wrong')).status).toBe(401);
    expect((await post()).status).toBe(401);
  });

  it('runs the reconciliation and answers its findings', async () => {
    process.env.FIAT_ADMIN_TOKEN = 'admin';
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    setFiatDepsForTest({ ledger });
    const alarms: string[] = [];
    setStripeReconcileForTest({
      source: source({ sessions: [session('pi_nowhere', 600, 100, { createdS: Math.floor(Date.now() / 1000) - 3600 })] }),
      onAlarm: (m) => alarms.push(m),
    });
    const res = await post('admin');
    expect(res.status).toBe(200);
    const body = (await res.json()) as { findings: string[]; sessions: number };
    expect(body.sessions).toBe(1);
    expect(body.findings).toHaveLength(1);
    expect(body.findings[0]).toMatch(/pi_nowhere/);
    expect(alarms).toHaveLength(1);
  });
});
