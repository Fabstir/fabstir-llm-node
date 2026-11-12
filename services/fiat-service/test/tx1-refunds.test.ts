// TX1.4 — refunds: across several card charges, with the VAT paid on the
// credits, one Stripe refund per charge under its own idempotency key, a
// look-up before compensating after an ambiguous error, and no double debit
// when Stripe's `charge.refunded` arrives for a refund this service issued.
// Assertions are on effects: ledger balances, per-charge figures, journal
// lines, and the exact refund requests (amount, key, metadata).
import { describe, expect, it } from 'vitest';
import { CreditsLedger, MemoryLedgerStore, cashoutLegKey } from '../src/lib/ledger';
import { FiatCredentials } from '../src/lib/fiat-credentials';
import { previewCashout, requestCashout, type CashoutDeps } from '../src/lib/fiat-cashout-service';
import { handleStripeEvent, type StripeWebhookDeps } from '../src/lib/stripe-webhook';
import { allowedBillingCountries } from '../src/lib/sales-countries';
import { POST as cashoutRoute } from '../app/v1/fiat/cashout/route';
import { setCashoutServiceForTest } from '../src/lib/fiat-cashout-service';
import { fakeStripeApi, stripeNoAnswer, stripeRefused, type FakeStripeOptions } from './helpers/fake-stripe';
import { chargeRefunded, disputeEvent, refundUpdated } from './fixtures/stripe-events';

const DAY_MS = 86_400_000;
const NOW = 1_794_000_000_000;
const USER = 'user-1';

interface Bought {
  pi: string;
  credits: number;
  /** VAT in cents on top of the credits' price. */
  taxCents: number;
  daysAgo?: number;
}

async function setup(purchases: Bought[], stripeOpts: FakeStripeOptions = {}) {
  let clock = NOW;
  const store = new MemoryLedgerStore();
  const ledger = await CreditsLedger.open(store, { now: () => clock });
  const credentials = await FiatCredentials.open(new MemoryLedgerStore());
  const token = await credentials.issue(USER);
  for (const [i, p] of purchases.entries()) {
    clock = NOW - (p.daysAgo ?? 0) * DAY_MS;
    await ledger.purchase(USER, BigInt(p.credits) * 10_000n, `evt_buy_${i}`, {
      paymentIntentId: p.pi,
      grossCents: p.credits + p.taxCents,
      taxCents: p.taxCents,
      billingCountry: 'GB',
    });
  }
  clock = NOW;
  const stripe = fakeStripeApi(stripeOpts);
  const alarms: string[] = [];
  let n = 0;
  const deps: CashoutDeps = {
    ledger,
    credentials,
    stripe: stripe.api,
    refundWindowDays: 90,
    now: () => NOW,
    newRequestId: () => `req${++n}`,
    onAlarm: (m) => alarms.push(m),
  };
  const webhookDeps: StripeWebhookDeps = {
    ledger,
    stripe: stripe.api,
    onAlarm: (m) => alarms.push(m),
    allowedCountries: allowedBillingCountries({}),
  };
  const journal = async () => (await store.load()).map((l) => JSON.parse(l) as Record<string, unknown>);
  const refund = (credits: number) => requestCashout(deps, { credential: token, amountMicro: BigInt(credits) * 10_000n });
  return { ledger, deps, token, stripe, alarms, journal, refund, webhookDeps };
}

const credits = (n: number) => BigInt(n) * 10_000n;

describe('TX1.4 refunds return the VAT paid on the credits', () => {
  it('one charge: 400 of 1,000 credits returns 480 cents (the 20% VAT on them included)', async () => {
    const h = await setup([{ pi: 'pi_1', credits: 1000, taxCents: 200 }]);
    const outcome = await h.refund(400);
    expect(outcome).toEqual({
      status: 'ok',
      refunds: [{ paymentIntentId: 'pi_1', refundId: 're_1', amountMicro: credits(400), grossCents: 480 }],
      failed: [],
      grossCents: 480,
      amountMicro: credits(400),
    });
    expect(h.stripe.calls).toEqual([
      {
        paymentIntentId: 'pi_1',
        amountCents: 480,
        idempotencyKey: 'cashout:req1:pi_1',
        metadata: { fc1Origin: 'cashout', fc1Key: 'cashout:req1:pi_1', fc1UserId: USER },
      },
    ]);
    expect(h.ledger.availableMicro(USER)).toBe(credits(600));
    expect(h.ledger.charge('pi_1')).toMatchObject({ refundedNetMicro: credits(400), refundedGrossCents: 480n });
    expect(h.ledger.cashoutLeg('cashout:req1:pi_1')).toMatchObject({ state: 'confirmed', refundId: 're_1' });
  });

  it('walks the purchases newest first across two charges, one Stripe refund per charge', async () => {
    const h = await setup([
      { pi: 'pi_old', credits: 500, taxCents: 100, daysAgo: 20 },
      { pi: 'pi_new', credits: 300, taxCents: 60, daysAgo: 2 },
    ]);
    const outcome = await h.refund(400);
    expect(outcome).toMatchObject({ status: 'ok', grossCents: 480, amountMicro: credits(400), failed: [] });
    // The newest charge is emptied (its whole 360 cents), the rest comes from the older one.
    expect(h.stripe.calls.map((c) => [c.paymentIntentId, c.amountCents, c.idempotencyKey])).toEqual([
      ['pi_new', 360, 'cashout:req1:pi_new'],
      ['pi_old', 120, 'cashout:req1:pi_old'],
    ]);
    expect(h.ledger.availableMicro(USER)).toBe(credits(400));
    expect(h.ledger.charge('pi_new')).toMatchObject({ remainingNetMicro: 0n, refundedGrossCents: 360n });
    expect(h.ledger.charge('pi_old')).toMatchObject({ remainingNetMicro: credits(400), refundedGrossCents: 120n });
    // ONE journal line carries both legs' debits.
    const lines = (await h.journal()).filter((l) => l.t === 'cashout-legs');
    expect(lines).toHaveLength(1);
    expect(lines[0]).toMatchObject({
      rid: 'req1',
      legs: [
        { pi: 'pi_new', amount: '3000000', grossCents: '360' },
        { pi: 'pi_old', amount: '1000000', grossCents: '120' },
      ],
    });
  });

  it('cumulative rounding: N partial refunds of one charge end at exactly the gross, no cent lost or made', async () => {
    // 999 credits at 20% VAT: Stripe charged 1,199 cents (199.8 rounds to 200).
    const h = await setup([{ pi: 'pi_r', credits: 999, taxCents: 200 }]);
    const parts = [111, 111, 111, 111, 111, 111, 111, 222];
    const gross: number[] = [];
    for (const part of parts) {
      const outcome = await h.refund(part);
      expect(outcome.status).toBe('ok');
      if (outcome.status === 'ok') gross.push(outcome.grossCents);
    }
    // Each leg is floor(1199 x cumulative / 999) - already refunded; the last
    // one returns exactly what is left.
    expect(gross).toEqual([133, 133, 133, 133, 134, 133, 133, 267]);
    expect(gross.reduce((a, b) => a + b, 0)).toBe(1199);
    expect(h.stripe.made.reduce((a, r) => a + r.amountCents!, 0)).toBe(1199);
    expect(h.ledger.charge('pi_r')).toMatchObject({ refundedGrossCents: 1199n, refundedNetMicro: credits(999), remainingNetMicro: 0n });
    expect(h.ledger.availableMicro(USER)).toBe(0n);
  });

  it('the refund window excludes an old charge: only charges inside 90 days are used', async () => {
    const h = await setup([
      { pi: 'pi_91', credits: 500, taxCents: 100, daysAgo: 91 },
      { pi: 'pi_10', credits: 300, taxCents: 60, daysAgo: 10 },
    ]);
    // 400 would need the 91-day-old charge: refused whole, nothing debited.
    expect(await h.refund(400)).toEqual({ status: 'refused', reason: 'EXCEEDS_REFUNDABLE' });
    expect(h.stripe.calls).toEqual([]);
    expect(h.ledger.availableMicro(USER)).toBe(credits(800));
    expect(await h.journal()).toHaveLength(2);
    // 300 fits in the recent charge.
    const outcome = await h.refund(300);
    expect(outcome).toMatchObject({ status: 'ok', grossCents: 360 });
    expect(h.stripe.calls.map((c) => c.paymentIntentId)).toEqual(['pi_10']);
    // Nothing recent is left: the old charge alone is outside the window.
    expect(await h.refund(100)).toEqual({ status: 'refused', reason: 'REFUND_WINDOW_EXPIRED' });
  });

  it('the preview shows the gross that will return, per charge, and changes nothing', async () => {
    const h = await setup([
      { pi: 'pi_old', credits: 500, taxCents: 100, daysAgo: 20 },
      { pi: 'pi_new', credits: 300, taxCents: 60, daysAgo: 2 },
    ]);
    const quote = await previewCashout(h.deps, { credential: h.token, amountMicro: credits(400) });
    expect(quote).toEqual({
      status: 'ok',
      grossCents: 480,
      amountMicro: credits(400),
      legs: [
        { paymentIntentId: 'pi_new', amountMicro: credits(300), grossCents: 360 },
        { paymentIntentId: 'pi_old', amountMicro: credits(100), grossCents: 120 },
      ],
    });
    expect(h.ledger.availableMicro(USER)).toBe(credits(800));
    expect(h.stripe.calls).toEqual([]);
    expect(await h.journal()).toHaveLength(2);
  });
});

describe('TX1.4 a Stripe error on a leg', () => {
  const two: Bought[] = [
    { pi: 'pi_old', credits: 500, taxCents: 100, daysAgo: 20 },
    { pi: 'pi_new', credits: 300, taxCents: 60, daysAgo: 2 },
  ];

  it('an AMBIGUOUS error on leg 2 looks the refund up first: it landed, so the debit stands (no cash AND credits)', async () => {
    const h = await setup(two, {
      onRefund: (_r, call) => (call === 2 ? { error: stripeNoAnswer('timeout after 20s'), landed: true } : undefined),
    });
    const outcome = await h.refund(400);
    expect(outcome).toMatchObject({ status: 'ok', grossCents: 480, amountMicro: credits(400), failed: [] });
    expect(h.stripe.lookups).toEqual([{ paymentIntentId: 'pi_old', key: 'cashout:req1:pi_old' }]);
    // The credits are NOT given back: the refund exists at Stripe.
    expect(h.ledger.availableMicro(USER)).toBe(credits(400));
    expect(h.ledger.cashoutLeg('cashout:req1:pi_old')).toMatchObject({ state: 'confirmed' });
    expect((await h.journal()).filter((l) => l.t === 'cashout-leg-reversed')).toEqual([]);
  });

  it('an ambiguous error whose look-up finds NO refund is compensated exactly; leg 1 stands', async () => {
    const h = await setup(two, {
      onRefund: (_r, call) => (call === 2 ? { error: stripeNoAnswer() } : undefined),
    });
    const outcome = await h.refund(400);
    expect(outcome).toMatchObject({ status: 'ok', grossCents: 360, amountMicro: credits(300) });
    expect(outcome.status === 'ok' && outcome.failed).toMatchObject([
      { paymentIntentId: 'pi_old', amountMicro: credits(100), grossCents: 120, state: 'reversed' },
    ]);
    expect(h.stripe.lookups).toHaveLength(1);
    // 300 refunded, the 100 of the failed leg back.
    expect(h.ledger.availableMicro(USER)).toBe(credits(500));
    expect(h.ledger.charge('pi_old')).toMatchObject({ refundedNetMicro: 0n, refundedGrossCents: 0n });
    expect(h.ledger.charge('pi_new')).toMatchObject({ refundedNetMicro: credits(300), refundedGrossCents: 360n });
  });

  it('when the look-up fails too the leg stays DEBITED and is reported unknown, with an alarm', async () => {
    const h = await setup(two, {
      onRefund: (_r, call) => (call === 2 ? { error: stripeNoAnswer(), landed: true } : undefined),
      lookupError: stripeNoAnswer('lookup timeout'),
    });
    const outcome = await h.refund(400);
    expect(outcome.status === 'ok' && outcome.failed).toMatchObject([{ paymentIntentId: 'pi_old', state: 'unknown' }]);
    expect(h.ledger.availableMicro(USER)).toBe(credits(400)); // both legs still debited
    expect(h.ledger.cashoutLeg('cashout:req1:pi_old')).toMatchObject({ state: 'pending' });
    expect(h.alarms.join('\n')).toMatch(/cashout:req1:pi_old/);
  });

  it('a DEFINITIVE refusal is compensated without a look-up; when every leg fails the answer is stripe_error', async () => {
    const h = await setup(two, { onRefund: () => ({ error: stripeRefused('charge is disputed') }) });
    const outcome = await h.refund(400);
    expect(outcome).toMatchObject({ status: 'stripe_error' });
    expect(h.stripe.lookups).toEqual([]);
    expect(h.ledger.availableMicro(USER)).toBe(credits(800));
    expect(h.ledger.charge('pi_new')).toMatchObject({ refundedNetMicro: 0n, refundedGrossCents: 0n, remainingNetMicro: credits(300) });
    expect(h.ledger.charge('pi_old')).toMatchObject({ refundedNetMicro: 0n, refundedGrossCents: 0n });
  });
});

describe('TX1.4 own refund and charge.refunded: one debit, in either order of arrival', () => {
  it('the API call returns, THEN the webhook arrives: nothing more is debited', async () => {
    const h = await setup([{ pi: 'pi_1', credits: 1000, taxCents: 200 }]);
    await h.refund(400);
    expect(h.ledger.availableMicro(USER)).toBe(credits(600));
    const res = await handleStripeEvent(
      h.webhookDeps,
      chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 480 })
    );
    expect(res.body).toMatchObject({ received: true, applied: false });
    expect(h.ledger.availableMicro(USER)).toBe(credits(600));
    expect(h.ledger.charge('pi_1')).toMatchObject({ refundedNetMicro: credits(400), refundedGrossCents: 480n, stripeSeenCents: 480n });
    expect(h.alarms).toEqual([]);
  });

  it('the webhook arrives WHILE the API call is still in flight: still one debit', async () => {
    let h!: Awaited<ReturnType<typeof setup>>;
    let balanceSeenByWebhook: bigint | undefined;
    h = await setup([{ pi: 'pi_1', credits: 1000, taxCents: 200 }], {
      onRefund: () => {
        // Stripe has made the refund and its event overtakes our own response.
        void handleStripeEvent(
          h.webhookDeps,
          chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 480 })
        ).then(() => {
          balanceSeenByWebhook = h.ledger.availableMicro(USER);
        });
        return undefined;
      },
    });
    const outcome = await h.refund(400);
    await new Promise((r) => setTimeout(r, 5));
    expect(outcome.status).toBe('ok');
    expect(balanceSeenByWebhook).toBe(credits(600));
    expect(h.ledger.availableMicro(USER)).toBe(credits(600));
    expect(h.ledger.charge('pi_1')).toMatchObject({ refundedNetMicro: credits(400), refundedGrossCents: 480n });
    expect((await h.journal()).filter((l) => l.t === 'stripe-refund' && l.amount !== '0')).toEqual([]);
  });

  it('a dashboard refund on top of an own refund debits only the dashboard part', async () => {
    const h = await setup([{ pi: 'pi_1', credits: 1000, taxCents: 200 }]);
    await h.refund(400); // 480 cents
    // Stripe now reports 720 refunded: 240 more, made in the dashboard = 200 credits.
    await handleStripeEvent(h.webhookDeps, chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 720 }));
    expect(h.ledger.availableMicro(USER)).toBe(credits(400));
    expect(h.ledger.charge('pi_1')).toMatchObject({ refundedNetMicro: credits(600), refundedGrossCents: 720n });
  });

  it('an own refund that later FAILS at the bank is credited back exactly, once', async () => {
    const h = await setup([{ pi: 'pi_1', credits: 999, taxCents: 200 }]);
    await h.refund(111); // 133 cents
    await h.refund(111); // 133 cents
    expect(h.ledger.availableMicro(USER)).toBe(credits(777));
    const failed = refundUpdated({
      eventId: 'evt_fail',
      refundId: 're_1',
      paymentIntent: 'pi_1',
      amountCents: 133,
      status: 'failed',
      metadata: { fc1Origin: 'cashout', fc1Key: cashoutLegKey('req1', 'pi_1') },
    });
    await handleStripeEvent(h.webhookDeps, failed);
    await handleStripeEvent(h.webhookDeps, failed);
    // Exactly the first leg's 111 credits, not a share worked back from cents.
    expect(h.ledger.availableMicro(USER)).toBe(credits(888));
    expect(h.ledger.charge('pi_1')).toMatchObject({ refundedNetMicro: credits(111), refundedGrossCents: 133n });
    expect(h.ledger.cashoutLeg(cashoutLegKey('req1', 'pi_1'))).toMatchObject({ state: 'reversed' });
    expect(h.alarms.join('\n')).toMatch(/re_1/);
  });
});

describe('TX1.4 disputed charges and frozen users', () => {
  it('a user with an open dispute is refused a refund, and nothing moves', async () => {
    const h = await setup([
      { pi: 'pi_a', credits: 500, taxCents: 100, daysAgo: 20 },
      { pi: 'pi_b', credits: 300, taxCents: 60, daysAgo: 2 },
    ]);
    await handleStripeEvent(
      h.webhookDeps,
      disputeEvent('charge.dispute.created', { disputeId: 'dp_1', paymentIntent: 'pi_b', amountCents: 360 })
    );
    expect(h.ledger.availableMicro(USER)).toBe(credits(500));
    expect(await h.refund(100)).toEqual({ status: 'refused', reason: 'ACCOUNT_FROZEN' });
    expect(h.stripe.calls).toEqual([]);
    expect(h.ledger.availableMicro(USER)).toBe(credits(500));
  });

  it('after a LOST dispute the disputed charge is skipped: the refund comes from another charge, never from it', async () => {
    const h = await setup([
      { pi: 'pi_a', credits: 500, taxCents: 100, daysAgo: 20 },
      { pi: 'pi_b', credits: 300, taxCents: 60, daysAgo: 2 },
    ]);
    // A partial dispute on the newest charge: 120 of its 360 cents (100 credits).
    await handleStripeEvent(
      h.webhookDeps,
      disputeEvent('charge.dispute.created', { disputeId: 'dp_1', paymentIntent: 'pi_b', amountCents: 120 })
    );
    await handleStripeEvent(
      h.webhookDeps,
      disputeEvent('charge.dispute.closed', { disputeId: 'dp_1', paymentIntent: 'pi_b', amountCents: 120, status: 'lost' })
    );
    expect(h.ledger.availableMicro(USER)).toBe(credits(700));
    const outcome = await h.refund(200);
    expect(outcome.status).toBe('ok');
    // pi_b still holds 200 undisputed credits, but it is never refunded again.
    expect(h.stripe.calls.map((c) => c.paymentIntentId)).toEqual(['pi_a']);
    // Only pi_a's 500 are refundable: 600 is more than that.
    expect(await h.refund(400)).toEqual({ status: 'refused', reason: 'EXCEEDS_REFUNDABLE' });
  });

  it('after a WON dispute the charge is refundable again', async () => {
    const h = await setup([{ pi: 'pi_a', credits: 500, taxCents: 100 }]);
    await handleStripeEvent(h.webhookDeps, disputeEvent('charge.dispute.created', { disputeId: 'dp_w', paymentIntent: 'pi_a', amountCents: 600 }));
    await handleStripeEvent(
      h.webhookDeps,
      disputeEvent('charge.dispute.closed', { disputeId: 'dp_w', paymentIntent: 'pi_a', amountCents: 600, status: 'won' })
    );
    const outcome = await h.refund(500);
    expect(outcome).toMatchObject({ status: 'ok', grossCents: 600 });
  });
});

describe('TX1.4 the cashout route', () => {
  function post(body: unknown, credential: string) {
    return cashoutRoute(
      new Request('http://fiat/v1/fiat/cashout', {
        method: 'POST',
        headers: { authorization: `Bearer ${credential}` },
        body: JSON.stringify(body),
      })
    );
  }

  it('answers { refunds[], grossCents, failed[] } and, with preview: true, the quote without moving anything', async () => {
    const h = await setup([
      { pi: 'pi_old', credits: 500, taxCents: 100, daysAgo: 20 },
      { pi: 'pi_new', credits: 300, taxCents: 60, daysAgo: 2 },
    ]);
    setCashoutServiceForTest({
      request: (r) => requestCashout(h.deps, r),
      preview: (r) => previewCashout(h.deps, r),
    });
    try {
      const quote = await post({ amountMicro: '4000000', preview: true }, h.token);
      expect(await quote.json()).toEqual({
        preview: true,
        amountMicro: '4000000',
        grossCents: 480,
        legs: [
          { paymentIntentId: 'pi_new', amountMicro: '3000000', grossCents: 360 },
          { paymentIntentId: 'pi_old', amountMicro: '1000000', grossCents: 120 },
        ],
      });
      expect(h.ledger.availableMicro(USER)).toBe(credits(800));

      const res = await post({ amountMicro: '4000000' }, h.token);
      expect(res.status).toBe(200);
      expect(await res.json()).toEqual({
        refunds: [
          { refundId: 're_1', paymentIntentId: 'pi_new', amountMicro: '3000000', grossCents: 360 },
          { refundId: 're_2', paymentIntentId: 'pi_old', amountMicro: '1000000', grossCents: 120 },
        ],
        grossCents: 480,
        failed: [],
        refundId: 're_1',
        amountMicro: '4000000',
      });
      expect(h.ledger.availableMicro(USER)).toBe(credits(400));
    } finally {
      setCashoutServiceForTest(undefined);
    }
  });
});
