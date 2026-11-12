// TX1 fix pass (2026-11-12): the findings of two independent reviews, each
// pinned by a test that asserts the effect (balances, journal lines, the exact
// Stripe requests), and each seen red under a mutation.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CreditsLedger, MemoryLedgerStore, cashoutLegKey } from '../src/lib/ledger';
import { FiatCredentials } from '../src/lib/fiat-credentials';
import {
  REPOST_WITHIN_MS,
  recoverPendingCashoutLegs,
  requestCashout,
  type CashoutDeps,
} from '../src/lib/fiat-cashout-service';
import { handleStripeEvent, type StripeWebhookDeps } from '../src/lib/stripe-webhook';
import { allowedBillingCountries } from '../src/lib/sales-countries';
import { STRIPE_API_VERSION, makeStripeApi, type StripePaymentState } from '../src/lib/stripe';
import { setStripeReconcileForTest } from '../src/lib/fiat-reconcile';
import { setFiatDepsForTest } from '../src/lib/fiat-session-service';
import { resetCheckoutRateLimitForTest } from '../src/lib/purchase-rate-limit';
import { POST as purchaseRoute } from '../app/v1/fiat/purchase/route';
import { POST as reconcileRoute } from '../app/v1/fiat/stripe/reconcile/route';
import { fakeStripeApi, stripeNoAnswer, stripeRefused, type FakeStripeOptions } from './helpers/fake-stripe';
import { USER, chargeRefunded, checkoutCompleted, disputeEvent, refundUpdated } from './fixtures/stripe-events';

const DAY_MS = 86_400_000;
const HOUR_MS = 3_600_000;
const MIN_MS = 60_000;
/** The fixtures stamp events at this unix second. */
const EVENT_S = 1_794_000_000;
const T0 = EVENT_S * 1000;
const credits = (n: number) => BigInt(n) * 10_000n;

async function world(stripeOpts: FakeStripeOptions = {}) {
  const store = new MemoryLedgerStore();
  const clock = { nowMs: T0 };
  const ledger = await CreditsLedger.open(store, { now: () => clock.nowMs });
  const stripe = fakeStripeApi(stripeOpts);
  const alarms: string[] = [];
  const webhook: StripeWebhookDeps = {
    ledger,
    stripe: stripe.api,
    onAlarm: (m) => alarms.push(m),
    allowedCountries: allowedBillingCountries({}),
    log: () => {},
    now: () => clock.nowMs,
  };
  const journal = async () => (await store.load()).map((l) => JSON.parse(l) as Record<string, unknown>);
  const buy = (pi: string, n = 1000, tax = 200) =>
    ledger.purchase(USER, credits(n), `evt_buy_${pi}`, { paymentIntentId: pi, grossCents: n + tax, taxCents: tax, sessionId: `cs_${pi}` });
  return { store, clock, ledger, stripe, alarms, webhook, journal, buy };
}

// ---------------------------------------------------------------------------
describe('M1 a refund leg left pending is recovered (boot and reconciliation)', () => {
  /** A crash after the debit was journaled and before Stripe answered. */
  async function crashed(stripeOpts: FakeStripeOptions = {}) {
    const w = await world(stripeOpts);
    await w.buy('pi_1');
    const plan = await w.ledger.planCashout(USER, credits(400), { requestId: 'r1', windowMs: 90 * DAY_MS });
    expect(plan.ok).toBe(true);
    // The process dies here. A new one opens the same journal.
    const reopened = await CreditsLedger.open(w.store, { now: () => w.clock.nowMs });
    expect(reopened.availableMicro(USER)).toBe(credits(600));
    expect(reopened.cashoutLeg(cashoutLegKey('r1', 'pi_1'))).toMatchObject({ state: 'pending' });
    const recover = (graceMs = 0) =>
      recoverPendingCashoutLegs({ ledger: reopened, stripe: w.stripe.api, onAlarm: (m) => w.alarms.push(m), now: () => w.clock.nowMs, graceMs });
    return { ...w, ledger: reopened, recover, key: cashoutLegKey('r1', 'pi_1') };
  }

  it('no refund at Stripe yet: it is made under the SAME idempotency key and the leg ends confirmed, debit standing', async () => {
    const w = await crashed();
    w.clock.nowMs = T0 + 30 * MIN_MS;
    const report = await w.recover();
    expect(report).toEqual({ confirmed: 0, reposted: 1, reversed: 0, stillPending: 0 });
    expect(w.stripe.lookups).toEqual([{ paymentIntentId: 'pi_1', key: w.key }]);
    expect(w.stripe.calls).toEqual([
      {
        paymentIntentId: 'pi_1',
        amountCents: 480,
        idempotencyKey: w.key,
        metadata: { fc1Origin: 'cashout', fc1Key: w.key, fc1UserId: USER },
      },
    ]);
    expect(w.ledger.cashoutLeg(w.key)).toMatchObject({ state: 'confirmed', refundId: 're_1' });
    expect(w.ledger.availableMicro(USER)).toBe(credits(600));
    expect(w.ledger.charge('pi_1')).toMatchObject({ refundedNetMicro: credits(400), refundedGrossCents: 480n });
  });

  it('the refund DID land before the crash: the leg is confirmed and no second refund is requested', async () => {
    let w!: Awaited<ReturnType<typeof crashed>>;
    w = await crashed();
    // The dead process's request reached Stripe.
    w.stripe.made.push({
      id: 're_landed',
      paymentIntentId: 'pi_1',
      amountCents: 480,
      idempotencyKey: w.key,
      metadata: { fc1Origin: 'cashout', fc1Key: w.key, fc1UserId: USER },
    });
    const report = await w.recover();
    expect(report).toMatchObject({ confirmed: 1, reposted: 0, reversed: 0 });
    expect(w.stripe.calls).toEqual([]);
    expect(w.ledger.cashoutLeg(w.key)).toMatchObject({ state: 'confirmed', refundId: 're_landed' });
    expect(w.ledger.availableMicro(USER)).toBe(credits(600));
  });

  it('past the idempotency window with no refund at Stripe: the leg is reversed and the credits are back, with no new refund', async () => {
    const w = await crashed();
    w.clock.nowMs = T0 + REPOST_WITHIN_MS + MIN_MS;
    const report = await w.recover();
    expect(report).toMatchObject({ confirmed: 0, reposted: 0, reversed: 1 });
    expect(w.stripe.calls).toEqual([]);
    expect(w.ledger.cashoutLeg(w.key)).toMatchObject({ state: 'reversed' });
    expect(w.ledger.availableMicro(USER)).toBe(credits(1000));
    expect(w.ledger.charge('pi_1')).toMatchObject({ refundedNetMicro: 0n, refundedGrossCents: 0n });
  });

  it('Stripe refuses the refund on recovery: reversed; no answer: left pending for the next sweep', async () => {
    const refused = await crashed({ onRefund: () => ({ error: stripeRefused('charge is disputed') }) });
    expect(await refused.recover()).toMatchObject({ reversed: 1, stillPending: 0 });
    expect(refused.ledger.availableMicro(USER)).toBe(credits(1000));

    const silent = await crashed({ onRefund: () => ({ error: stripeNoAnswer() }) });
    expect(await silent.recover()).toMatchObject({ reversed: 0, reposted: 0, stillPending: 1 });
    expect(silent.ledger.cashoutLeg(silent.key)).toMatchObject({ state: 'pending' });
    expect(silent.ledger.availableMicro(USER)).toBe(credits(600)); // never credits AND cash
    expect(silent.alarms.join('\n')).toMatch(/still pending/);

    const blind = await crashed({ lookupError: stripeNoAnswer('lookup timeout') });
    expect(await blind.recover()).toMatchObject({ stillPending: 1 });
    expect(blind.stripe.calls).toEqual([]); // no refund is made blind
  });

  it('a leg inside the grace period (a request may still be in flight) is not touched', async () => {
    const w = await crashed();
    w.clock.nowMs = T0 + 5 * MIN_MS;
    expect(await w.recover(15 * MIN_MS)).toEqual({ confirmed: 0, reposted: 0, reversed: 0, stillPending: 1 });
    expect(w.stripe.lookups).toEqual([]);
    expect(w.stripe.calls).toEqual([]);
  });

  it('a leg whose look-up could not answer in the request path is settled by the next sweep', async () => {
    const w = await world({ onRefund: () => ({ error: stripeNoAnswer(), landed: true }), lookupError: stripeNoAnswer() });
    await w.buy('pi_1');
    const credentials = await FiatCredentials.open(new MemoryLedgerStore());
    const token = await credentials.issue(USER);
    const deps: CashoutDeps = {
      ledger: w.ledger,
      credentials,
      stripe: w.stripe.api,
      refundWindowDays: 90,
      now: () => w.clock.nowMs,
      newRequestId: () => 'r1',
      onAlarm: () => {},
    };
    const outcome = await requestCashout(deps, { credential: token, amountMicro: credits(400) });
    expect(outcome.status).toBe('stripe_error');
    expect(w.ledger.cashoutLeg(cashoutLegKey('r1', 'pi_1'))).toMatchObject({ state: 'pending' });
    // Stripe answers again later: the refund is there.
    const later = fakeStripeApi();
    later.made.push(...w.stripe.made);
    const report = await recoverPendingCashoutLegs({ ledger: w.ledger, stripe: later.api, onAlarm: () => {}, now: () => w.clock.nowMs + HOUR_MS, graceMs: 15 * MIN_MS });
    expect(report).toMatchObject({ confirmed: 1 });
    expect(later.calls).toEqual([]);
    expect(w.ledger.availableMicro(USER)).toBe(credits(600));
  });

  it('the reconcile route runs the recovery before it compares', async () => {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore(), { now: () => Date.now() - HOUR_MS });
    await ledger.purchase(USER, credits(1000), 'evt_1', { paymentIntentId: 'pi_1', grossCents: 1200, taxCents: 200 });
    await ledger.planCashout(USER, credits(400), { requestId: 'r1', windowMs: 90 * DAY_MS });
    const stripe = fakeStripeApi();
    process.env.FIAT_ADMIN_TOKEN = 'admin';
    setFiatDepsForTest({ ledger });
    setStripeReconcileForTest({
      source: { listPaidSessions: async () => [], listRefunds: async () => [], listDisputes: async () => [] },
      onAlarm: () => {},
      stripe: stripe.api,
    });
    try {
      const res = await reconcileRoute(
        new Request('http://fiat/v1/fiat/stripe/reconcile', { method: 'POST', headers: { authorization: 'Bearer admin' } })
      );
      expect(res.status).toBe(200);
      expect(((await res.json()) as { pendingLegs: unknown }).pendingLegs).toEqual({ confirmed: 0, reposted: 1, reversed: 0, stillPending: 0 });
      expect(ledger.cashoutLeg(cashoutLegKey('r1', 'pi_1'))).toMatchObject({ state: 'confirmed' });
      expect(stripe.calls.map((c) => c.idempotencyKey)).toEqual([cashoutLegKey('r1', 'pi_1')]);
    } finally {
      delete process.env.FIAT_ADMIN_TOKEN;
      setFiatDepsForTest(undefined);
      setStripeReconcileForTest(undefined);
    }
  });
});

// ---------------------------------------------------------------------------
describe('M2 an event that overtakes its purchase is redelivered, not dropped', () => {
  const OURS = { app: 'platformless-ai', fc1UserId: USER };

  it('a dashboard refund BEFORE the delayed purchase event: 503 until the purchase is journaled, then debited', async () => {
    const w = await world();
    w.clock.nowMs = T0 + 10 * MIN_MS;
    const refund = chargeRefunded({ eventId: 'evt_r', paymentIntent: 'pi_late', amountCents: 1200, amountRefundedCents: 600, metadata: OURS });
    const early = await handleStripeEvent(w.webhook, refund);
    expect(early.status).toBe(503);
    expect(early.body).toMatchObject({ retry: true });
    expect(await w.journal()).toEqual([]);
    expect(w.alarms).toEqual([]);

    // The purchase event arrives late and credits in full.
    await handleStripeEvent(w.webhook, checkoutCompleted({ paymentIntent: 'pi_late', credits: 1000, taxCents: 200 }));
    expect(w.ledger.availableMicro(USER)).toBe(credits(1000));

    // Stripe redelivers the refund event: now it is applied.
    const again = await handleStripeEvent(w.webhook, refund);
    expect(again.status).toBe(200);
    expect(w.ledger.availableMicro(USER)).toBe(credits(500));
  });

  it('the usual order (purchase, then refund) is unchanged', async () => {
    const w = await world();
    await handleStripeEvent(w.webhook, checkoutCompleted({ paymentIntent: 'pi_n', credits: 1000, taxCents: 200 }));
    const res = await handleStripeEvent(
      w.webhook,
      chargeRefunded({ paymentIntent: 'pi_n', amountCents: 1200, amountRefundedCents: 600, metadata: OURS })
    );
    expect(res.status).toBe(200);
    expect(w.ledger.availableMicro(USER)).toBe(credits(500));
  });

  it('after six hours an unknown charge of ours is alarmed and let go; another product\'s is ignored at once', async () => {
    const w = await world();
    w.clock.nowMs = T0 + 6 * HOUR_MS + MIN_MS;
    const old = await handleStripeEvent(
      w.webhook,
      chargeRefunded({ paymentIntent: 'pi_never', amountCents: 1200, amountRefundedCents: 600, metadata: OURS })
    );
    expect(old.status).toBe(200);
    expect(w.alarms.join('\n')).toMatch(/pi_never/);

    const v = await world();
    v.clock.nowMs = T0 + MIN_MS;
    const other = await handleStripeEvent(
      v.webhook,
      chargeRefunded({ paymentIntent: 'pi_v2', amountCents: 900, amountRefundedCents: 900, metadata: { app: 'fabstir-v2' } })
    );
    expect(other.status).toBe(200);
    expect(v.alarms).toEqual([]);
    expect(v.stripe.stateReads).toEqual([]); // tagged as another product's: no read needed
  });

  it('a dispute BEFORE the delayed purchase event: whose it is comes from Stripe; ours is redelivered, then debited', async () => {
    const states: Record<string, StripePaymentState | Error> = {
      pi_late: { app: 'platformless-ai', userId: USER, createdS: EVENT_S },
      pi_v2: { app: 'fabstir-v2', createdS: EVENT_S },
      pi_down: new Error('stripe timeout'),
    };
    const w = await world({ paymentState: (pi) => states[pi] ?? {} });
    w.clock.nowMs = T0 + 10 * MIN_MS;
    const dispute = disputeEvent('charge.dispute.created', { eventId: 'evt_d', disputeId: 'dp_1', paymentIntent: 'pi_late', amountCents: 1200 });
    expect((await handleStripeEvent(w.webhook, dispute)).status).toBe(503);
    expect((await handleStripeEvent(w.webhook, disputeEvent('charge.dispute.created', { disputeId: 'dp_2', paymentIntent: 'pi_v2', amountCents: 900 }))).status).toBe(200);
    // Whose charge it is cannot be read: ask again rather than drop it.
    expect((await handleStripeEvent(w.webhook, disputeEvent('charge.dispute.created', { disputeId: 'dp_3', paymentIntent: 'pi_down', amountCents: 900 }))).status).toBe(503);
    expect(await w.journal()).toEqual([]);

    await handleStripeEvent(w.webhook, checkoutCompleted({ paymentIntent: 'pi_late', credits: 1000, taxCents: 200 }));
    expect((await handleStripeEvent(w.webhook, dispute)).status).toBe(200);
    expect(w.ledger.availableMicro(USER)).toBe(0n);
    expect(w.ledger.isFrozen(USER)).toBe(true);
  });

  it('a failed refund on a charge not journaled yet is redelivered too', async () => {
    const w = await world({ paymentState: () => ({ app: 'platformless-ai', createdS: EVENT_S }) });
    w.clock.nowMs = T0 + MIN_MS;
    const res = await handleStripeEvent(w.webhook, refundUpdated({ paymentIntent: 'pi_late', amountCents: 300, status: 'failed' }));
    expect(res.status).toBe(503);
  });
});

// ---------------------------------------------------------------------------
describe('M3 a paid session that cannot be credited is refunded, never dropped', () => {
  it('paid in a currency other than US dollars: refunded in full under the gate key, reason currency, with an alarm', async () => {
    const w = await world();
    const res = await handleStripeEvent(w.webhook, checkoutCompleted({ currency: 'gbp', sessionId: 'cs_gbp', paymentIntent: 'pi_gbp', credits: 1000, taxCents: 200 }));
    expect(res.status).toBe(200);
    expect(res.body).toMatchObject({ refused: 'currency', refunded: true });
    expect(w.ledger.availableMicro(USER)).toBe(0n);
    expect(w.stripe.calls).toHaveLength(1);
    expect(w.stripe.calls[0]).toMatchObject({ paymentIntentId: 'pi_gbp', idempotencyKey: 'gate:cs_gbp' });
    expect(w.stripe.calls[0]!.amountCents).toBeUndefined();
    expect(w.stripe.calls[0]!.metadata).toMatchObject({ fc1Origin: 'gate', fc1Reason: 'currency' });
    expect((await w.journal())[0]).toMatchObject({ t: 'purchase-refused', reason: 'currency', pi: 'pi_gbp', grossCents: '1200' });
    expect(w.alarms.join('\n')).toMatch(/cannot be credited/);
    // Redelivered: refunded once.
    const event = checkoutCompleted({ eventId: 'evt_gbp2', currency: 'gbp', sessionId: 'cs_gbp', paymentIntent: 'pi_gbp' });
    await handleStripeEvent(w.webhook, event);
    expect(w.stripe.calls).toHaveLength(1);
  });

  it('a tax figure that is not below the total: refunded, reason tax_amount', async () => {
    const w = await world();
    const event = checkoutCompleted({ paymentIntent: 'pi_tax', credits: 1000, taxCents: 200 }) as unknown as {
      data: { object: { total_details: { amount_tax: number } } };
    };
    event.data.object.total_details.amount_tax = 1200; // the whole total
    const res = await handleStripeEvent(w.webhook, event);
    expect(res.body).toMatchObject({ refused: 'tax_amount', refunded: true });
    expect(w.ledger.availableMicro(USER)).toBe(0n);
    expect(w.stripe.calls).toHaveLength(1);
  });

  it('a session carrying our tag but no user id: refunded, reason no_user; another product\'s session is left alone', async () => {
    const w = await world();
    const ours = checkoutCompleted({ paymentIntent: 'pi_nouser', sessionId: 'cs_nouser' }) as unknown as { data: { object: { metadata: Record<string, string> } } };
    delete ours.data.object.metadata.fc1UserId;
    ours.data.object.metadata.app = 'platformless-ai';
    const res = await handleStripeEvent(w.webhook, ours);
    expect(res.body).toMatchObject({ refused: 'no_user', refunded: true });
    expect(w.stripe.calls.map((c) => c.idempotencyKey)).toEqual(['gate:cs_nouser']);
    expect(w.stripe.calls[0]!.metadata).not.toHaveProperty('fc1UserId');

    const theirs = checkoutCompleted({ paymentIntent: 'pi_v2' }) as unknown as { data: { object: { metadata: Record<string, string> } } };
    theirs.data.object.metadata = { app: 'fabstir-v2' };
    const ignored = await handleStripeEvent(w.webhook, theirs);
    expect(ignored.body).toMatchObject({ ignored: expect.any(String) });
    expect(w.stripe.calls).toHaveLength(1);
  });

  it('a session that is not paid is still just ignored', async () => {
    const w = await world();
    const res = await handleStripeEvent(w.webhook, checkoutCompleted({ currency: 'gbp', paymentStatus: 'unpaid' }));
    expect(res.body).toMatchObject({ ignored: expect.any(String) });
    expect(w.stripe.calls).toEqual([]);
    expect(await w.journal()).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
describe('M4 a refund that failed never comes back as a debit', () => {
  async function ownRefundThatFailed(stripeOpts: FakeStripeOptions = {}) {
    const w = await world(stripeOpts);
    await w.buy('pi_1');
    const credentials = await FiatCredentials.open(new MemoryLedgerStore());
    const token = await credentials.issue(USER);
    let n = 0;
    const cashout: CashoutDeps = {
      ledger: w.ledger,
      credentials,
      stripe: w.stripe.api,
      refundWindowDays: 90,
      now: () => w.clock.nowMs,
      newRequestId: () => `r${++n}`,
      onAlarm: () => {},
    };
    const refund = (c: number) => requestCashout(cashout, { credential: token, amountMicro: credits(c) });
    return { ...w, refund };
  }
  const failedEvent = (key: string) =>
    refundUpdated({ refundId: 're_1', paymentIntent: 'pi_1', amountCents: 480, status: 'failed', metadata: { fc1Origin: 'cashout', fc1Key: key } });

  it('own refund seen by Stripe, then fails; a LATER leg that Stripe refuses is reversed without a phantom debit', async () => {
    let refuse = false;
    const w = await ownRefundThatFailed({ onRefund: () => (refuse ? { error: stripeRefused() } : undefined) });
    await w.refund(400); // 480 cents, confirmed
    await handleStripeEvent(w.webhook, chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 480 }));
    expect(w.ledger.charge('pi_1')).toMatchObject({ stripeSeenCents: 480n });
    await handleStripeEvent(w.webhook, failedEvent(cashoutLegKey('r1', 'pi_1')));
    expect(w.ledger.availableMicro(USER)).toBe(credits(1000));
    // Stripe's cumulative figure dropped with the failure; the ledger's copy must too.
    expect(w.ledger.charge('pi_1')).toMatchObject({ stripeSeenCents: 0n, refundedGrossCents: 0n, refundFailures: 1 });

    refuse = true;
    const second = await w.refund(100);
    expect(second.status).toBe('stripe_error');
    // All 1,000 credits: the reversal's re-check must not debit 400 for the refund that failed.
    expect(w.ledger.availableMicro(USER)).toBe(credits(1000));
    expect((await w.journal()).filter((l) => l.t === 'stripe-refund' && l.amount !== '0')).toEqual([]);
  });

  it('a STALE charge.refunded snapshot delivered after the failure debits nothing: the charge\'s current state is read', async () => {
    const now: StripePaymentState = { app: 'platformless-ai', amountRefundedCents: 0 };
    const w = await ownRefundThatFailed({ paymentState: () => now });
    await w.refund(400);
    await handleStripeEvent(w.webhook, failedEvent(cashoutLegKey('r1', 'pi_1')));
    expect(w.ledger.availableMicro(USER)).toBe(credits(1000));
    // The snapshot taken while the refund still looked made arrives only now.
    const stale = await handleStripeEvent(w.webhook, chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 480 }));
    expect(stale.status).toBe(200);
    // Read twice: once when the failure was recorded, once for the stale snapshot.
    expect(w.stripe.stateReads).toEqual(['pi_1', 'pi_1']);
    expect(w.ledger.availableMicro(USER)).toBe(credits(1000));
    expect(w.ledger.charge('pi_1')).toMatchObject({ refundedNetMicro: 0n, refundedGrossCents: 0n });
  });

  it('after a failure a REAL later refund is still debited, on the current figure', async () => {
    const now: StripePaymentState = { app: 'platformless-ai', amountRefundedCents: 240 };
    const w = await ownRefundThatFailed({ paymentState: () => now });
    await w.refund(400);
    await handleStripeEvent(w.webhook, failedEvent(cashoutLegKey('r1', 'pi_1')));
    // A stale snapshot says 480; the truth now is a 240-cent dashboard refund.
    await handleStripeEvent(w.webhook, chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 480 }));
    expect(w.ledger.availableMicro(USER)).toBe(credits(800));
    expect(w.ledger.charge('pi_1')).toMatchObject({ refundedGrossCents: 240n });
  });

  it('when the current state cannot be read the snapshot is not trusted: 503, nothing debited', async () => {
    const w = await ownRefundThatFailed({ paymentState: () => new Error('stripe timeout') });
    await w.refund(400);
    await handleStripeEvent(w.webhook, failedEvent(cashoutLegKey('r1', 'pi_1')));
    const res = await handleStripeEvent(w.webhook, chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 480 }));
    expect(res.status).toBe(503);
    expect(w.ledger.availableMicro(USER)).toBe(credits(1000));
  });

  it('a charge with no failed refund is not read from Stripe (the snapshot can only understate)', async () => {
    const w = await world();
    await w.buy('pi_1');
    await handleStripeEvent(w.webhook, chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 600 }));
    expect(w.stripe.stateReads).toEqual([]);
    expect(w.ledger.availableMicro(USER)).toBe(credits(500));
  });

  it('a cancelled refund is treated as one that never reached the card', async () => {
    const w = await world();
    await w.buy('pi_1');
    await handleStripeEvent(w.webhook, chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 600 }));
    await handleStripeEvent(w.webhook, refundUpdated({ refundId: 're_c', paymentIntent: 'pi_1', amountCents: 600, status: 'canceled' }));
    expect(w.ledger.availableMicro(USER)).toBe(credits(1000));
  });
});

// ---------------------------------------------------------------------------
describe('M5 a dispute that closes always closes: the user is never left frozen', () => {
  for (const status of ['prevented', 'charge_refunded']) {
    it(`closed as ${status} (the cardholder has the money back): the debit stands and the user is unfrozen`, async () => {
      const w = await world();
      await w.buy('pi_1');
      await handleStripeEvent(w.webhook, disputeEvent('charge.dispute.created', { disputeId: 'dp', paymentIntent: 'pi_1', amountCents: 1200, status: 'warning_needs_response' }));
      expect(w.ledger.isFrozen(USER)).toBe(true);
      w.alarms.length = 0;
      await handleStripeEvent(w.webhook, disputeEvent('charge.dispute.closed', { disputeId: 'dp', paymentIntent: 'pi_1', amountCents: 1200, status }));
      expect(w.ledger.isFrozen(USER)).toBe(false);
      expect(w.ledger.availableMicro(USER)).toBe(0n);
      expect(w.ledger.dispute('dp')).toMatchObject({ state: 'lost' });
      expect(w.ledger.charge('pi_1')).toMatchObject({ remainingNetMicro: 0n });
      expect(w.alarms).toEqual([]);
    });
  }

  it('a closing status this service does not know: closed with the debit standing, and an alarm', async () => {
    const w = await world();
    await w.buy('pi_1');
    await handleStripeEvent(w.webhook, disputeEvent('charge.dispute.created', { disputeId: 'dp', paymentIntent: 'pi_1', amountCents: 1200 }));
    w.alarms.length = 0;
    await handleStripeEvent(w.webhook, disputeEvent('charge.dispute.closed', { disputeId: 'dp', paymentIntent: 'pi_1', amountCents: 1200, status: 'something_new' }));
    expect(w.ledger.isFrozen(USER)).toBe(false);
    expect(w.ledger.availableMicro(USER)).toBe(0n);
    expect(w.alarms.join('\n')).toMatch(/something_new/);
  });
});

// ---------------------------------------------------------------------------
describe('M7 one Checkout Session is one purchase, whatever the event ids', () => {
  it('a second event object for the same session is not credited again and calls Stripe for nothing', async () => {
    const w = await world();
    const first = checkoutCompleted({ eventId: 'evt_a', sessionId: 'cs_same', paymentIntent: 'pi_same' });
    const twin = checkoutCompleted({ eventId: 'evt_b', sessionId: 'cs_same', paymentIntent: 'pi_same' });
    await handleStripeEvent(w.webhook, first);
    const lookups = w.stripe.countryLookups.length;
    const res = await handleStripeEvent(w.webhook, twin);
    expect(res.body).toMatchObject({ applied: false, duplicate: 'session' });
    expect(w.ledger.availableMicro(USER)).toBe(credits(1000));
    expect(w.stripe.countryLookups).toHaveLength(lookups);
    expect((await w.journal()).filter((l) => l.t === 'purchase')).toHaveLength(1);
  });

  it('completed, then async_payment_succeeded for the same PaymentIntent under another session id: still one purchase', async () => {
    const w = await world();
    await handleStripeEvent(w.webhook, checkoutCompleted({ sessionId: 'cs_1', paymentIntent: 'pi_same' }));
    const res = await handleStripeEvent(
      w.webhook,
      checkoutCompleted({ type: 'checkout.session.async_payment_succeeded', sessionId: 'cs_other', paymentIntent: 'pi_same' })
    );
    expect(res.body).toMatchObject({ applied: false, duplicate: 'payment_intent' });
    expect(w.ledger.availableMicro(USER)).toBe(credits(1000));
  });

  it('two event objects for one session handled CONCURRENTLY credit once', async () => {
    const w = await world();
    const results = await Promise.all([
      handleStripeEvent(w.webhook, checkoutCompleted({ eventId: 'evt_a', sessionId: 'cs_same', paymentIntent: 'pi_same' })),
      handleStripeEvent(w.webhook, checkoutCompleted({ eventId: 'evt_b', sessionId: 'cs_same', paymentIntent: 'pi_same' })),
    ]);
    expect(results.map((r) => r.body.applied).sort()).toEqual([false, true]);
    expect(w.ledger.availableMicro(USER)).toBe(credits(1000));
    expect(w.ledger.charge('pi_same')).toMatchObject({ netMicro: credits(1000), grossCents: 1200n });
  });

  it('two refused event objects for one session, concurrently: journaled once, one idempotency key', async () => {
    const w = await world();
    await Promise.all([
      handleStripeEvent(w.webhook, checkoutCompleted({ eventId: 'evt_a', sessionId: 'cs_us', paymentIntent: 'pi_us', country: 'US', taxCents: 0 })),
      handleStripeEvent(w.webhook, checkoutCompleted({ eventId: 'evt_b', sessionId: 'cs_us', paymentIntent: 'pi_us', country: 'US', taxCents: 0 })),
    ]);
    expect((await w.journal()).filter((l) => l.t === 'purchase-refused')).toHaveLength(1);
    expect(new Set(w.stripe.calls.map((c) => c.idempotencyKey))).toEqual(new Set(['gate:cs_us']));
    expect(w.ledger.availableMicro(USER)).toBe(0n);
  });
});

// ---------------------------------------------------------------------------
describe('M8 an amount is validated before it is journaled', () => {
  it('a bad cents value is refused and nothing is appended (it would stop the ledger opening on replay)', async () => {
    const w = await world();
    await w.buy('pi_1');
    await w.ledger.disputeOpened({ disputeId: 'dp', paymentIntentId: 'pi_1', amountCents: 600, eventId: 'evt_dp' });
    const before = (await w.store.load()).length;
    for (const bad of [1.5, -1, Number.NaN, Number.POSITIVE_INFINITY, '600' as unknown as number]) {
      await expect(w.ledger.disputeFunds({ disputeId: 'dp', kind: 'withdrawn', cents: bad, eventId: `evt_${String(bad)}` })).rejects.toThrow(/cents/);
      await expect(
        w.ledger.recordRefusedPurchase({ userId: USER, eventId: `evt_r_${String(bad)}`, paymentIntentId: 'pi_x', grossCents: bad, taxCents: 0, reason: 'country' })
      ).rejects.toThrow(/cents/);
      await expect(
        w.ledger.refundFailed({ refundId: `re_${String(bad)}`, paymentIntentId: 'pi_1', amountCents: bad, eventId: `evt_f_${String(bad)}` })
      ).rejects.toThrow(/cents/);
      await expect(
        w.ledger.purchase(USER, 1_000_000n, `evt_p_${String(bad)}`, { paymentIntentId: 'pi_y', grossCents: bad, taxCents: 0 })
      ).rejects.toThrow(/cents/);
      await expect(w.ledger.chargeRefunded('pi_1', bad, `evt_c_${String(bad)}`)).rejects.toThrow(/cents/);
    }
    expect((await w.store.load()).length).toBe(before);
    // And the journal still opens.
    await expect(CreditsLedger.open(w.store)).resolves.toBeDefined();
  });
});

// ---------------------------------------------------------------------------
describe('S1 every Stripe request is pinned to one API version', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    delete process.env.FIAT_STRIPE_SECRET_KEY;
  });

  it('refunds, reads and Checkout all send Stripe-Version 2026-08-26.dahlia', async () => {
    expect(STRIPE_API_VERSION).toBe('2026-08-26.dahlia');
    process.env.FIAT_STRIPE_SECRET_KEY = 'sk_test_x';
    const seen: Array<{ url: string; version: string | undefined }> = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, init?: RequestInit) => {
        seen.push({ url, version: (init?.headers as Record<string, string> | undefined)?.['stripe-version'] });
        return new Response(JSON.stringify({ id: 'x_1', url: 'https://checkout.stripe.com/c/x', data: [], metadata: {} }), { status: 200 });
      })
    );
    const api = makeStripeApi();
    await api.createRefund({ paymentIntentId: 'pi_1', idempotencyKey: 'k', metadata: {} });
    await api.cardCountry('pi_1');
    await api.paymentState('pi_1');
    await api.findRefund('pi_1', 'k');
    resetCheckoutRateLimitForTest();
    await purchaseRoute(
      new Request('http://fiat/v1/fiat/purchase', {
        method: 'POST',
        body: JSON.stringify({
          clientAddress: '0x1234567890abcdef1234567890abcdef12345678',
          credits: 500,
          consent: { termsVersion: '2026-10-23.1', over18: true, useNow: true },
        }),
      })
    );
    expect(seen).toHaveLength(5);
    for (const call of seen) expect(call.version, call.url).toBe('2026-08-26.dahlia');
  });
});

// ---------------------------------------------------------------------------
describe('R4 the refund look-up reads every page', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    delete process.env.FIAT_STRIPE_SECRET_KEY;
  });

  it('finds a refund by its key on the SECOND page, and concludes "none" only after the last page', async () => {
    process.env.FIAT_STRIPE_SECRET_KEY = 'sk_test_x';
    const page = (from: number, hasMore: boolean, extra: Array<Record<string, unknown>> = []) => ({
      object: 'list',
      has_more: hasMore,
      data: [...Array.from({ length: 100 }, (_, i) => ({ id: `re_${from + i}`, status: 'succeeded', metadata: {} })), ...extra],
    });
    const urls: string[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        urls.push(url);
        const second = url.includes('starting_after=re_99');
        const body = second
          ? page(100, false, [{ id: 're_target', status: 'succeeded', metadata: { fc1Key: 'cashout:r1:pi_1' } }])
          : page(0, true);
        return new Response(JSON.stringify(body), { status: 200 });
      })
    );
    const api = makeStripeApi();
    expect(await api.findRefund('pi_1', 'cashout:r1:pi_1')).toEqual({ id: 're_target', status: 'succeeded' });
    expect(urls).toHaveLength(2);
    expect(urls[0]).toContain('payment_intent=pi_1');
    expect(urls[1]).toContain('starting_after=re_99');
    // A key that is on no page: undefined, after both pages were read.
    urls.length = 0;
    expect(await api.findRefund('pi_1', 'cashout:r2:pi_1')).toBeUndefined();
    expect(urls).toHaveLength(2);
  });
});

// ---------------------------------------------------------------------------
describe('M6 and S6: the purchase route', () => {
  const CONSENT = { termsVersion: '2026-10-23.1', over18: true, useNow: true };
  const addr = (n: number) => `0x${n.toString(16).padStart(40, '0')}`;
  let stripeFails = false;
  let fetchMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    process.env.FIAT_STRIPE_SECRET_KEY = 'sk_test_x';
    resetCheckoutRateLimitForTest();
    stripeFails = false;
    fetchMock = vi.fn(async () =>
      stripeFails
        ? new Response(JSON.stringify({ error: { message: 'stripe is down' } }), { status: 500 })
        : new Response(JSON.stringify({ id: 'cs_1', url: 'https://checkout.stripe.com/c/cs_1' }), { status: 200 })
    );
    vi.stubGlobal('fetch', fetchMock);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    delete process.env.FIAT_STRIPE_SECRET_KEY;
    delete process.env.FIAT_STRIPE_TOS_CONSENT;
    delete process.env.FIAT_MAX_CHECKOUTS_PER_HOUR_PER_IP;
  });

  function post(clientAddress: string, forwardedFor?: string) {
    return purchaseRoute(
      new Request('http://fiat/v1/fiat/purchase', {
        method: 'POST',
        headers: forwardedFor ? { 'x-forwarded-for': forwardedFor } : {},
        body: JSON.stringify({ clientAddress, credits: 500, consent: CONSENT }),
      })
    );
  }

  it('one client address rotating account addresses is limited (10 an hour by default)', async () => {
    for (let i = 0; i < 10; i++) expect((await post(addr(i + 1), '203.0.113.7')).status).toBe(200);
    const eleventh = await post(addr(99), '203.0.113.7');
    expect(eleventh.status).toBe(429);
    expect(fetchMock).toHaveBeenCalledTimes(10);
    // Another client is not affected.
    expect((await post(addr(100), '198.51.100.9')).status).toBe(200);
  });

  it('only the address our own proxy appended counts: entries a client put in the header change nothing', async () => {
    process.env.FIAT_MAX_CHECKOUTS_PER_HOUR_PER_IP = '2';
    expect((await post(addr(1), '1.1.1.1, 203.0.113.7')).status).toBe(200);
    expect((await post(addr(2), '2.2.2.2, 203.0.113.7')).status).toBe(200);
    expect((await post(addr(3), '3.3.3.3, 203.0.113.7')).status).toBe(429);
  });

  it('a loopback peer (no proxy in front, or a caller on the box) is not keyed by address', async () => {
    process.env.FIAT_MAX_CHECKOUTS_PER_HOUR_PER_IP = '2';
    for (let i = 0; i < 4; i++) expect((await post(addr(i + 1), '127.0.0.1')).status).toBe(200);
    for (let i = 0; i < 4; i++) expect((await post(addr(i + 10))).status).toBe(200);
  });

  it('a Stripe failure gives the start back: five 502s do not lock the buyer out', async () => {
    stripeFails = true;
    for (let i = 0; i < 6; i++) expect((await post(addr(1), '203.0.113.7')).status).toBe(502);
    stripeFails = false;
    for (let i = 0; i < 5; i++) expect((await post(addr(1), '203.0.113.7')).status).toBe(200);
    expect((await post(addr(1), '203.0.113.7')).status).toBe(429);
  });

  it('FIAT_STRIPE_TOS_CONSENT=on adds Stripe\'s own required checkbox restating the three statements; off by default', async () => {
    await post(addr(1));
    const off = new URLSearchParams((fetchMock.mock.calls[0]![1] as RequestInit).body as string);
    expect(off.has('consent_collection[terms_of_service]')).toBe(false);
    expect(off.has('custom_text[terms_of_service_acceptance][message]')).toBe(false);

    process.env.FIAT_STRIPE_TOS_CONSENT = 'on';
    await post(addr(2));
    const on = new URLSearchParams((fetchMock.mock.calls[1]![1] as RequestInit).body as string);
    expect(on.get('consent_collection[terms_of_service]')).toBe('required');
    const message = on.get('custom_text[terms_of_service_acceptance][message]')!;
    expect(message).toContain('Terms of Service');
    expect(message).toContain('I am 18 or over.');
    expect(message).toContain('I want to use my credits straight away.');
    expect(message).toContain('Unused credits stay refundable.');
    expect(message.length).toBeLessThanOrEqual(1200);
  });

  it('tags the session itself as ours, so a paid session that lost its user id can be recognised and refunded', async () => {
    await post(addr(1));
    const form = new URLSearchParams((fetchMock.mock.calls[0]![1] as RequestInit).body as string);
    expect(form.get('metadata[app]')).toBe('platformless-ai');
  });
});
