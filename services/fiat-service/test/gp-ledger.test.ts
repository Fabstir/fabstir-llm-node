// GP4 — the ledger in two currencies: the currency journaled and replayed, an
// old journal read as US dollars, refunds of a pound charge (the 0p leg of A1,
// the exact sum to the gross), each refund row's VAT share (A13), a refund
// spanning a dollar and a pound charge, and the reconciliation's R14 view.
import { describe, expect, it } from 'vitest';
import { CreditsLedger, MemoryLedgerStore } from '../src/lib/ledger';
import { FiatCredentials } from '../src/lib/fiat-credentials';
import { ZERO_LEG_REFUND_ID, previewCashout, recoverPendingCashoutLegs, requestCashout, type CashoutDeps } from '../src/lib/fiat-cashout-service';
import { purchaseHistory } from '../src/lib/purchase-history';
import { reconcileStripe, type StripeRefundRow, type StripeSessionRow } from '../src/lib/fiat-reconcile';
import { fakeStripeApi, stripeRefused, type FakeStripeOptions } from './helpers/fake-stripe';
import { servedFx, ALWAYS_133 } from './helpers/fx-feed';

const DAY_MS = 86_400_000;
const NOW = 1_794_000_000_000;
const USER = 'user-1';
const micro = (c: number) => BigInt(c) * 10_000n;

interface Bought {
  pi: string;
  credits: number;
  grossCents: number;
  taxCents: number;
  currency?: string;
  daysAgo?: number;
}
const POUNDS_10K: Omit<Bought, 'pi'> = { credits: 10_000, grossCents: 9526, taxCents: 1588, currency: 'gbp' };

async function setup(purchases: Bought[], stripeOpts: FakeStripeOptions = {}) {
  let clock = NOW;
  const store = new MemoryLedgerStore();
  const ledger = await CreditsLedger.open(store, { now: () => clock });
  const credentials = await FiatCredentials.open(new MemoryLedgerStore());
  const token = await credentials.issue(USER);
  for (const [i, p] of purchases.entries()) {
    clock = NOW - (p.daysAgo ?? 0) * DAY_MS;
    await ledger.purchase(USER, micro(p.credits), `evt_buy_${i}`, {
      paymentIntentId: p.pi,
      grossCents: p.grossCents,
      taxCents: p.taxCents,
      ...(p.currency ? { currency: p.currency } : {}),
    });
  }
  clock = NOW;
  const stripe = fakeStripeApi(stripeOpts);
  let n = 0;
  const deps: CashoutDeps = {
    ledger, credentials, stripe: stripe.api, refundWindowDays: 90, now: () => NOW,
    newRequestId: () => `req${++n}`, onAlarm: () => {},
  };
  const refund = (c: number) => requestCashout(deps, { credential: token, amountMicro: micro(c) });
  const journal = async () => (await store.load()).map((l) => JSON.parse(l) as Record<string, unknown>);
  return { ledger, store, stripe, deps, token, refund, journal };
}

describe('GP4 the currency in the journal', () => {
  it('is journaled with a pound purchase and read back on replay; an old line reads as US dollars', async () => {
    const h = await setup([{ pi: 'pi_gbp', ...POUNDS_10K }, { pi: 'pi_old', credits: 1000, grossCents: 1200, taxCents: 200 }]);
    const lines = await h.journal();
    expect(lines[0]).toMatchObject({ t: 'purchase', pi: 'pi_gbp', currency: 'gbp' });
    expect(lines[1]).not.toHaveProperty('currency'); // no currency given: the same line as before GP
    const again = await CreditsLedger.open(h.store);
    expect(again.charge('pi_gbp')!.currency).toBe('gbp');
    expect(again.charge('pi_old')!.currency).toBe('usd');
  });

  it('a pre-GP journal opens unchanged, charges and refusals in US dollars', async () => {
    const store = new MemoryLedgerStore();
    await store.append(JSON.stringify({ t: 'purchase', userId: USER, amount: '10000000', eventId: 'e1', pi: 'pi_1', at: 1, grossCents: '1200', taxCents: '200' }));
    await store.append(JSON.stringify({ t: 'purchase-refused', userId: USER, eventId: 'e2', pi: 'pi_2', grossCents: '500', taxCents: '0', reason: 'country', refunded: true, at: 2 }));
    const ledger = await CreditsLedger.open(store);
    expect(ledger.charge('pi_1')).toMatchObject({ currency: 'usd', grossCents: 1200n });
    expect(ledger.refusedPurchases()[0]).toMatchObject({ currency: 'usd' });
    expect(purchaseHistory(ledger, USER).purchases[0]).toMatchObject({ currency: 'usd', netCents: 1000 });
  });

  it('a refused purchase journals its currency (A3)', async () => {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    await ledger.recordRefusedPurchase({ userId: USER, eventId: 'e', paymentIntentId: 'pi_r', grossCents: 985, taxCents: 164, reason: 'rate_window', refunded: true, currency: 'gbp' });
    expect(ledger.refusedPurchases()[0]).toMatchObject({ currency: 'gbp', reason: 'rate_window' });
    expect(purchaseHistory(ledger, USER).automaticRefunds[0]).toMatchObject({ currency: 'gbp', grossCents: 985 });
  });
});

describe('GP4 refunds of a pound charge (A1)', () => {
  it('a request that returns 0p is refused refund_too_small and moves nothing', async () => {
    const h = await setup([{ pi: 'pi_1', ...POUNDS_10K }]);
    // 1 credit of 10,000 for 9,526p: floor(0.95) = 0p.
    expect(await h.refund(1)).toEqual({ status: 'refused', reason: 'REFUND_TOO_SMALL' });
    expect(h.ledger.availableMicro(USER)).toBe(micro(10_000));
    expect(h.stripe.calls).toEqual([]);
    expect(await previewCashout(h.deps, { credential: h.token, amountMicro: micro(1) })).toEqual({ status: 'refused', reason: 'REFUND_TOO_SMALL' });
  });

  it('a 0p leg beside a leg that returns money: the credits move, no Stripe refund for it, and the penny comes back later', async () => {
    const h = await setup([
      { pi: 'pi_old', ...POUNDS_10K, daysAgo: 2 },
      { pi: 'pi_new', ...POUNDS_10K, daysAgo: 1 },
    ]);
    const out = await h.refund(10_001); // all of pi_new, and 1 credit of pi_old (0p)
    expect(out).toMatchObject({ status: 'ok', grossCents: 9526, byCurrency: [{ currency: 'gbp', grossMinor: 9526 }] });
    expect(h.stripe.calls.map((c) => [c.paymentIntentId, c.amountCents])).toEqual([['pi_new', 9526]]);
    if (out.status === 'ok') {
      expect(out.refunds).toContainEqual({ paymentIntentId: 'pi_old', refundId: ZERO_LEG_REFUND_ID, amountMicro: micro(1), grossCents: 0, currency: 'gbp' });
      expect(out.refunds[0]!.paymentIntentId).toBe('pi_new'); // the money leg first
    }
    expect(h.ledger.cashoutLeg('cashout:req1:pi_old')).toMatchObject({ state: 'confirmed', refundId: ZERO_LEG_REFUND_ID, grossCents: 0n });
    // Emptying the old charge returns its whole gross: nothing was lost.
    await h.refund(9_999);
    expect(h.stripe.calls.map((c) => [c.paymentIntentId, c.amountCents])).toEqual([['pi_new', 9526], ['pi_old', 9526]]);
    expect(h.ledger.charge('pi_old')).toMatchObject({ refundedGrossCents: 9526n, refundedNetMicro: micro(10_000) });
  });

  it('when every money leg fails, the 0p leg is reversed with them: no credits kept for nothing', async () => {
    const h = await setup(
      [{ pi: 'pi_old', ...POUNDS_10K, daysAgo: 2 }, { pi: 'pi_new', ...POUNDS_10K, daysAgo: 1 }],
      { onRefund: () => ({ error: stripeRefused() }) }
    );
    const out = await h.refund(10_001);
    expect(out.status).toBe('stripe_error');
    expect(h.ledger.availableMicro(USER)).toBe(micro(20_000));
    expect(h.ledger.cashoutLeg('cashout:req1:pi_old')!.state).toBe('reversed');
  });

  it('a 0p leg left pending by a crash is confirmed by the recovery sweep without calling Stripe', async () => {
    const h = await setup([{ pi: 'pi_old', ...POUNDS_10K, daysAgo: 2 }, { pi: 'pi_new', ...POUNDS_10K, daysAgo: 1 }]);
    const plan = await h.ledger.planCashout(USER, micro(10_001), { requestId: 'crash', windowMs: 90 * DAY_MS, nowMs: NOW });
    expect(plan.ok).toBe(true);
    await h.ledger.confirmCashoutLeg('cashout:crash:pi_new', 're_x');
    const report = await recoverPendingCashoutLegs({ ledger: h.ledger, stripe: h.stripe.api, onAlarm: () => {}, graceMs: 0, now: () => NOW });
    expect(report).toMatchObject({ confirmed: 1, reposted: 0, reversed: 0 });
    expect(h.stripe.calls).toEqual([]);
    expect(h.stripe.lookups).toEqual([]);
  });

  it('a 0p leg left pending beside a money leg the sweep reverses is reversed too, not kept for nothing', async () => {
    const h = await setup([{ pi: 'pi_old', ...POUNDS_10K, daysAgo: 2 }, { pi: 'pi_new', ...POUNDS_10K, daysAgo: 1 }]);
    const plan = await h.ledger.planCashout(USER, micro(10_001), { requestId: 'crash', windowMs: 90 * DAY_MS, nowMs: NOW });
    expect(plan.ok).toBe(true);
    await h.ledger.reverseCashoutLeg('cashout:crash:pi_new', 'stripe_refused');
    const report = await recoverPendingCashoutLegs({ ledger: h.ledger, stripe: h.stripe.api, onAlarm: () => {}, graceMs: 0, now: () => NOW });
    expect(report).toMatchObject({ confirmed: 0, reversed: 1 });
    expect(h.ledger.cashoutLeg('cashout:crash:pi_old')!.state).toBe('reversed');
    expect(h.ledger.availableMicro(USER)).toBe(micro(20_000));
  });

  it('partial refunds of the 10,000 pack sum exactly to its gross, and its VAT rows to its VAT (A13)', async () => {
    const h = await setup([{ pi: 'pi_1', ...POUNDS_10K }]);
    let left = 10_000;
    for (const step of [2, 3, 7, 1999, 2, 5000]) {
      expect((await h.refund(step)).status).toBe('ok');
      left -= step;
    }
    expect((await h.refund(left)).status).toBe('ok');
    const legs = h.ledger.allCashoutLegs();
    expect(legs.every((l) => l.grossCents > 0n)).toBe(true);
    expect(legs.reduce((s, l) => s + l.grossCents, 0n)).toBe(9526n);
    expect(h.stripe.calls.reduce((s, c) => s + (c.amountCents ?? 0), 0)).toBe(9526);
    const rows = purchaseHistory(h.ledger, USER).refunds;
    expect(rows.every((r) => r.currency === 'gbp')).toBe(true);
    expect(rows.reduce((s, r) => s + r.taxCents, 0)).toBe(1588);
    expect(rows.reduce((s, r) => s + r.netCents + r.taxCents, 0)).toBe(9526);
  });

  it('many small refunds never show more VAT than the charge carried', async () => {
    const h = await setup([{ pi: 'pi_1', ...POUNDS_10K }]);
    for (let i = 0; i < 400; i++) expect((await h.refund(3)).status).toBe('ok');
    const part = purchaseHistory(h.ledger, USER).refunds;
    const gross = part.reduce((s, r) => s + r.grossCents, 0);
    const tax = part.reduce((s, r) => s + r.taxCents, 0);
    expect(Math.abs(tax - Math.round((1588 * gross) / 9526))).toBeLessThanOrEqual(1);
    expect((await h.refund(10_000 - 1200)).status).toBe('ok');
    const rows = purchaseHistory(h.ledger, USER).refunds;
    expect(rows.reduce((s, r) => s + r.taxCents, 0)).toBe(1588);
    expect(rows.every((r) => r.taxCents >= 0 && r.taxCents <= r.grossCents)).toBe(true);
  });

  it('a refund row\'s VAT is the charge\'s share, not gross minus credits', async () => {
    const h = await setup([{ pi: 'pi_1', ...POUNDS_10K }]);
    await h.refund(2000); // a fifth: floor(9526 / 5) = 1905p
    const [row] = purchaseHistory(h.ledger, USER).refunds;
    // round-half-up(1588 x 1905 / 9526) = 318 (gross - credits would say 1905 - 2000 < 0).
    expect(row).toMatchObject({ grossCents: 1905, taxCents: 318, netCents: 1587, currency: 'gbp', creditsMicro: String(micro(2000)) });
  });
});

describe('GP4 a refund spanning an old dollar charge and a pound charge', () => {
  it('is previewed, refunded and listed per currency', async () => {
    const h = await setup([
      { pi: 'pi_usd', credits: 500, grossCents: 600, taxCents: 100, daysAgo: 30 },
      { pi: 'pi_gbp', credits: 500, grossCents: 510, taxCents: 85, currency: 'gbp', daysAgo: 1 },
    ]);
    const quote = await previewCashout(h.deps, { credential: h.token, amountMicro: micro(600) });
    expect(quote).toMatchObject({
      status: 'ok',
      byCurrency: [{ currency: 'gbp', grossMinor: 510 }, { currency: 'usd', grossMinor: 120 }],
      legs: [
        { paymentIntentId: 'pi_gbp', grossCents: 510, currency: 'gbp' },
        { paymentIntentId: 'pi_usd', grossCents: 120, currency: 'usd' },
      ],
    });
    const out = await h.refund(600);
    expect(out).toMatchObject({ status: 'ok', byCurrency: [{ currency: 'gbp', grossMinor: 510 }, { currency: 'usd', grossMinor: 120 }] });
    const rows = purchaseHistory(h.ledger, USER).refunds;
    expect(rows.map((r) => [r.reference, r.currency, r.grossCents, r.taxCents]).sort()).toEqual([
      ['pi_gbp', 'gbp', 510, 85],
      ['pi_usd', 'usd', 120, 20],
    ]);
  });
});

describe('GP4 reconcile (R14-aware)', () => {
  const S = Math.floor(NOW / 1000) - 3600;
  const stamp = { pack: 1000, fxMicro: 1_330_000, priceMinor: 985, createdS: S, expiresAtS: S + 1860 };
  const row = (pi: string, over: Partial<StripeSessionRow> = {}): StripeSessionRow => ({
    sessionId: `cs_${pi}`, paymentIntentId: pi, userId: USER, grossCents: 985, taxCents: 164, currency: 'gbp', createdS: S,
    stamp, pricing: 'gbp1', ...over,
  });
  async function run(ledger: CreditsLedger, sessions: StripeSessionRow[], lines = ALWAYS_133, refunds: StripeRefundRow[] = []) {
    const alarms: string[] = [];
    await reconcileStripe({
      ledger,
      source: { listPaidSessions: async () => sessions, listRefunds: async () => refunds, listDisputes: async () => [] },
      onAlarm: (m) => alarms.push(m),
      now: () => NOW,
      lookbackMs: 100 * DAY_MS,
      fx: servedFx(lines),
    });
    return alarms;
  }

  it('agrees on a pound charge credited the pack', async () => {
    const h = await setup([{ pi: 'pi_1', credits: 1000, grossCents: 985, taxCents: 164, currency: 'gbp', daysAgo: 0 }]);
    expect(await run(h.ledger, [row('pi_1')])).toEqual([]);
  });

  it('flags a pound charge credited the wrong amount', async () => {
    const h = await setup([{ pi: 'pi_1', credits: 1200, grossCents: 985, taxCents: 164, currency: 'gbp' }]);
    const alarms = await run(h.ledger, [row('pi_1')]);
    expect(alarms.join('\n')).toMatch(/pi_1: Stripe has gross 985.*expected credit 10000000 micro/);
  });

  it('expects a refund for an R14 failure: silent when refused, flagged when credited or missing', async () => {
    const outside: ServedLines = [{ at: 0, fxMicro: 1_300_000 }];
    const refused = await setup([]);
    await refused.ledger.recordRefusedPurchase({ userId: USER, eventId: 'e', paymentIntentId: 'pi_r', grossCents: 985, taxCents: 164, reason: 'rate_window', refunded: true, currency: 'gbp' });
    const gateRefund: StripeRefundRow = { refundId: 're_g', paymentIntentId: 'pi_r', amountCents: 985, status: 'succeeded', origin: 'gate' };
    expect(await run(refused.ledger, [row('pi_r')], outside, [gateRefund])).toEqual([]);

    const credited = await setup([{ pi: 'pi_c', credits: 1000, grossCents: 985, taxCents: 164, currency: 'gbp' }]);
    expect((await run(credited.ledger, [row('pi_c')], outside)).join('\n')).toMatch(/pi_c: CREDITED .* refund owed: .*R14 b/);

    const missing = await setup([]);
    expect((await run(missing.ledger, [row('pi_m', { stamp: { ...stamp, expiresAtS: S + 1921 } })])).join('\n')).toMatch(/refund owed .*R14 c.*never refunded/);
  });

  it('an old dollar charge is still compared the old way', async () => {
    const h = await setup([{ pi: 'pi_usd', credits: 1000, grossCents: 1200, taxCents: 200 }]);
    expect(await run(h.ledger, [row('pi_usd', { grossCents: 1200, taxCents: 200, currency: 'usd', stamp: undefined, pricing: undefined })])).toEqual([]);
  });
});

type ServedLines = Parameters<typeof servedFx>[0];
