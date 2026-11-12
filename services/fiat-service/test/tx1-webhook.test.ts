// TX1.2 — the webhook: VAT, the country gate, location evidence, refunds made
// outside this service, refunds that fail, and chargebacks. Every assertion is
// on the EFFECT (balances, journal lines, the refund request and its
// idempotency key), never on a status code alone. The Stripe events are
// constructed fixtures (see fixtures/stripe-events.ts).
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { CreditsLedger, MemoryLedgerStore } from '../src/lib/ledger';
import { handleStripeEvent, setStripeWebhookForTest, type StripeWebhookDeps } from '../src/lib/stripe-webhook';
import { StripeApiError, stripeSignatureHeader, type StripeApi, type StripePaymentState } from '../src/lib/stripe';
import { allowedBillingCountries } from '../src/lib/sales-countries';
import { setFiatDepsForTest } from '../src/lib/fiat-session-service';
import { makeGatekeeper } from '../src/lib/gatekeeper';
import { POST as webhookRoute } from '../app/v1/fiat/stripe/webhook/route';
import { USER, chargeRefunded, checkoutCompleted, disputeEvent, refundUpdated } from './fixtures/stripe-events';

const HOST = '0x00000000000000000000000000000000000000aa';

interface RefundCall {
  paymentIntentId: string;
  amountCents?: number;
  idempotencyKey: string;
  metadata: Record<string, string>;
}

type StripeOpts = {
  cardCountry?: string | Error;
  refundError?: () => Error | undefined;
  paymentState?: (pi: string) => StripePaymentState | Error;
};

function fakeStripe(opts: StripeOpts = {}) {
  const refunds: RefundCall[] = [];
  const countryLookups: Array<{ pi: string; timeoutMs?: number }> = [];
  const stateReads: string[] = [];
  const api: StripeApi = {
    async createRefund(p) {
      const err = opts.refundError?.();
      if (err) throw err;
      refunds.push(p);
      return { id: `re_gate_${refunds.length}`, status: 'succeeded' };
    },
    async cardCountry(pi, o) {
      countryLookups.push({ pi, ...(o?.timeoutMs !== undefined ? { timeoutMs: o.timeoutMs } : {}) });
      if (opts.cardCountry instanceof Error) throw opts.cardCountry;
      return opts.cardCountry ?? 'GB';
    },
    async paymentState(pi) {
      stateReads.push(pi);
      const state = opts.paymentState?.(pi) ?? {};
      if (state instanceof Error) throw state;
      return state;
    },
    async findRefund() {
      return undefined;
    },
  };
  return { api, refunds, countryLookups, stateReads };
}

/** The fixtures' events are stamped 1,794,000,000 (unix seconds); "now" is
 *  seven hours later unless a test says otherwise. */
const EVENT_S = 1_794_000_000;
const HOUR_MS = 3_600_000;

async function harness(opts: StripeOpts & { salesEu?: boolean; nowMs?: number } = {}) {
  const store = new MemoryLedgerStore();
  const ledger = await CreditsLedger.open(store, { now: () => 1_794_000_000_000 });
  const { api, refunds, countryLookups, stateReads } = fakeStripe(opts);
  const alarms: string[] = [];
  const clock = { nowMs: opts.nowMs ?? EVENT_S * 1000 + 7 * HOUR_MS };
  const deps: StripeWebhookDeps = {
    ledger,
    stripe: api,
    onAlarm: (m) => alarms.push(m),
    allowedCountries: allowedBillingCountries({ SALES_EU: opts.salesEu ? 'on' : 'off' }),
    log: () => {},
    now: () => clock.nowMs,
  };
  const journal = async () => (await store.load()).map((l) => JSON.parse(l) as Record<string, unknown>);
  return { deps, ledger, refunds, alarms, journal, store, countryLookups, stateReads, clock };
}

describe('TX1.2 purchase: credited on the amount before tax', () => {
  it('a UK consumer is credited the net and the journal carries gross, tax, both countries and the consent', async () => {
    const h = await harness();
    const event = checkoutCompleted({ credits: 1000, taxCents: 200, sessionId: 'cs_uk', paymentIntent: 'pi_uk' });
    const res = await handleStripeEvent(h.deps, event);
    expect(res.status).toBe(200);
    expect(res.body).toMatchObject({ received: true, applied: true });
    // 1,000 credits, not 1,200: the VAT is not spendable.
    expect(h.ledger.availableMicro(USER)).toBe(10_000_000n);
    expect(h.refunds).toEqual([]);
    expect(h.alarms).toEqual([]);
    const [line] = await h.journal();
    expect(line).toMatchObject({
      t: 'purchase',
      userId: USER,
      amount: '10000000',
      pi: 'pi_uk',
      cs: 'cs_uk',
      grossCents: '1200',
      taxCents: '200',
      country: 'GB',
      cardCountry: 'GB',
      taxId: false,
      terms: '2026-10-23.1',
      over18: true,
      useNow: true,
    });
    expect(h.ledger.charge('pi_uk')).toMatchObject({ netMicro: 10_000_000n, grossCents: 1200n, taxCents: 200n });
  });

  it('records the "use straight away" statement exactly as the session carries it (a session made before it became required)', async () => {
    const h = await harness();
    await handleStripeEvent(h.deps, checkoutCompleted({ consent: { useNow: '0' } }));
    expect((await h.journal())[0]).toMatchObject({ t: 'purchase', useNow: false, over18: true });
  });

  it('a UK business with a GB VAT number is still charged VAT and credited the net (no reverse charge at home)', async () => {
    const h = await harness();
    const event = checkoutCompleted({
      credits: 2500,
      taxCents: 500,
      paymentIntent: 'pi_b2b',
      taxIds: [{ type: 'gb_vat', value: 'GB123456789' }],
    });
    await handleStripeEvent(h.deps, event);
    expect(h.ledger.availableMicro(USER)).toBe(25_000_000n);
    expect((await h.journal())[0]).toMatchObject({ taxId: true, taxCents: '500', grossCents: '3000' });
    expect(h.alarms).toEqual([]);
  });

  it('a replayed event credits once and calls Stripe for nothing the second time', async () => {
    const h = await harness();
    const event = checkoutCompleted({ eventId: 'evt_replay' });
    await handleStripeEvent(h.deps, event);
    expect(h.countryLookups).toHaveLength(1);
    const again = await handleStripeEvent(h.deps, event);
    expect(again.body).toMatchObject({ received: true, applied: false });
    expect(h.ledger.availableMicro(USER)).toBe(10_000_000n);
    expect(await h.journal()).toHaveLength(1);
    // The early guard: without it the card-country read would run again.
    expect(h.countryLookups).toHaveLength(1);
  });

  it('reads the card country with a short timeout: the webhook must answer Stripe quickly', async () => {
    const h = await harness();
    await handleStripeEvent(h.deps, checkoutCompleted({ paymentIntent: 'pi_t' }));
    expect(h.countryLookups).toEqual([{ pi: 'pi_t', timeoutMs: 5000 }]);
  });
});

describe('TX1.2 the country gate (billing address)', () => {
  it('a US billing address is refused: nothing credited, the whole charge refunded under key gate:<session id>', async () => {
    const h = await harness();
    const event = checkoutCompleted({ country: 'US', taxCents: 0, sessionId: 'cs_us', paymentIntent: 'pi_us' });
    const res = await handleStripeEvent(h.deps, event);
    expect(res.status).toBe(200);
    expect(res.body).toMatchObject({ received: true, refused: 'country', refunded: true });
    expect(h.ledger.availableMicro(USER)).toBe(0n);
    expect(h.refunds).toHaveLength(1);
    expect(h.refunds[0]).toMatchObject({ paymentIntentId: 'pi_us', idempotencyKey: 'gate:cs_us' });
    // The whole charge: no partial amount.
    expect(h.refunds[0]!.amountCents).toBeUndefined();
    expect(h.refunds[0]!.metadata).toMatchObject({ fc1Origin: 'gate' });
    const [line] = await h.journal();
    expect(line).toMatchObject({ t: 'purchase-refused', pi: 'pi_us', cs: 'cs_us', country: 'US', reason: 'country', grossCents: '1000' });
    expect(h.ledger.charge('pi_us')).toBeUndefined();
  });

  it('an EU billing address is refused while SALES_EU is off and credited when it is on', async () => {
    const off = await harness({ salesEu: false });
    await handleStripeEvent(off.deps, checkoutCompleted({ country: 'IE', taxCents: 230, paymentIntent: 'pi_ie' }));
    expect(off.ledger.availableMicro(USER)).toBe(0n);
    expect(off.refunds).toHaveLength(1);

    const on = await harness({ salesEu: true, cardCountry: 'IE' });
    await handleStripeEvent(on.deps, checkoutCompleted({ country: 'IE', taxCents: 230, paymentIntent: 'pi_ie' }));
    expect(on.ledger.availableMicro(USER)).toBe(10_000_000n);
    expect(on.refunds).toEqual([]);
    expect((await on.journal())[0]).toMatchObject({ country: 'IE', taxCents: '230', grossCents: '1230' });
  });

  it('a US address stays refused with SALES_EU on, and a missing address is refused too', async () => {
    const h = await harness({ salesEu: true });
    await handleStripeEvent(h.deps, checkoutCompleted({ country: 'US', taxCents: 0 }));
    await handleStripeEvent(h.deps, checkoutCompleted({ country: null as unknown as string, taxCents: 0 }));
    expect(h.ledger.availableMicro(USER)).toBe(0n);
    expect(h.refunds).toHaveLength(2);
  });

  it('a replay of a refused event refunds once', async () => {
    const h = await harness();
    const event = checkoutCompleted({ country: 'US', taxCents: 0, eventId: 'evt_us_replay' });
    await handleStripeEvent(h.deps, event);
    const again = await handleStripeEvent(h.deps, event);
    expect(again.body).toMatchObject({ applied: false });
    expect(h.refunds).toHaveLength(1);
    expect(await h.journal()).toHaveLength(1);
  });

  it('a refund that fails answers 5xx and journals nothing, so the retry refunds and journals', async () => {
    let fail = true;
    const h = await harness({ refundError: () => (fail ? new StripeApiError('stripe down', 503, undefined) : undefined) });
    const event = checkoutCompleted({ country: 'US', taxCents: 0, eventId: 'evt_retry', sessionId: 'cs_retry' });
    const first = await handleStripeEvent(h.deps, event);
    expect(first.status).toBeGreaterThanOrEqual(500);
    expect(await h.journal()).toEqual([]);
    expect(h.alarms.join('\n')).toMatch(/refund/i);

    fail = false;
    const second = await handleStripeEvent(h.deps, event);
    expect(second.status).toBe(200);
    expect(h.refunds).toHaveLength(1);
    expect(h.refunds[0]!.idempotencyKey).toBe('gate:cs_retry');
    expect(await h.journal()).toHaveLength(1);
    expect(h.ledger.availableMicro(USER)).toBe(0n);
  });

  it('charge_already_refunded on a late retry counts as refunded (the key has expired, the refund has not)', async () => {
    const h = await harness({
      refundError: () => new StripeApiError('Charge has already been refunded.', 400, 'charge_already_refunded'),
    });
    const res = await handleStripeEvent(h.deps, checkoutCompleted({ country: 'US', taxCents: 0, paymentIntent: 'pi_late' }));
    expect(res.status).toBe(200);
    expect((await h.journal())[0]).toMatchObject({ t: 'purchase-refused', pi: 'pi_late', refunded: true });
    expect(h.ledger.availableMicro(USER)).toBe(0n);
  });
});

describe('TX1.2 tax status and alarms', () => {
  it('automatic_tax.status other than complete is refused and refunded, with an alarm', async () => {
    for (const status of ['requires_location_inputs', 'failed', null]) {
      const h = await harness();
      const res = await handleStripeEvent(h.deps, checkoutCompleted({ taxStatus: status, sessionId: 'cs_tax' }));
      expect(res.body).toMatchObject({ refused: 'tax_status' });
      expect(h.ledger.availableMicro(USER)).toBe(0n);
      expect(h.refunds).toHaveLength(1);
      expect(h.refunds[0]!.idempotencyKey).toBe('gate:cs_tax');
      expect(h.alarms.join('\n')).toMatch(/automatic_tax/);
      expect((await h.journal())[0]).toMatchObject({ t: 'purchase-refused', reason: 'tax_status' });
    }
  });

  it('alarms when a GB purchase carries zero VAT (registration or tax code missing), and still credits', async () => {
    const h = await harness();
    await handleStripeEvent(h.deps, checkoutCompleted({ credits: 500, taxCents: 0 }));
    expect(h.ledger.availableMicro(USER)).toBe(5_000_000n);
    expect(h.alarms).toHaveLength(1);
    expect(h.alarms[0]).toMatch(/zero VAT/i);
  });

  it('alarms when the card country and the billing country disagree, credits by the billing address and journals both', async () => {
    const h = await harness({ cardCountry: 'US' });
    await handleStripeEvent(h.deps, checkoutCompleted({ paymentIntent: 'pi_mismatch' }));
    expect(h.ledger.availableMicro(USER)).toBe(10_000_000n);
    expect(h.alarms).toHaveLength(1);
    expect(h.alarms[0]).toMatch(/card.*US.*billing.*GB|billing.*GB.*card.*US/i);
    expect((await h.journal())[0]).toMatchObject({ country: 'GB', cardCountry: 'US' });
  });

  it('a failed card-country lookup still credits, alarms, and journals the billing country alone', async () => {
    const h = await harness({ cardCountry: new Error('stripe timeout') });
    await handleStripeEvent(h.deps, checkoutCompleted({}));
    expect(h.ledger.availableMicro(USER)).toBe(10_000_000n);
    expect(h.alarms.join('\n')).toMatch(/card country/i);
    const [line] = await h.journal();
    expect(line).toMatchObject({ country: 'GB' });
    expect(line).not.toHaveProperty('cardCountry');
  });

  it('alarms when the subtotal Stripe charged is not the credits that were bought', async () => {
    const h = await harness();
    await handleStripeEvent(h.deps, checkoutCompleted({ credits: 1000, subtotalCents: 900, taxCents: 180 }));
    // Credited what was actually paid before tax.
    expect(h.ledger.availableMicro(USER)).toBe(9_000_000n);
    expect(h.alarms.join('\n')).toMatch(/subtotal/i);
  });
});

async function withPurchase(opts: { credits?: number; taxCents?: number; pi?: string } = {}) {
  const h = await harness();
  await handleStripeEvent(
    h.deps,
    checkoutCompleted({ credits: opts.credits ?? 1000, taxCents: opts.taxCents ?? 200, paymentIntent: opts.pi ?? 'pi_1' })
  );
  h.alarms.length = 0;
  return h;
}

describe('TX1.2 charge.refunded: refunds made outside this service', () => {
  it('a dashboard refund is debited as its net share, exactly once across replays', async () => {
    const h = await withPurchase(); // 1,000 credits for 1,200 cents
    const event = chargeRefunded({ eventId: 'evt_dash', paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 600 });
    const res = await handleStripeEvent(h.deps, event);
    expect(res.body).toMatchObject({ received: true, applied: true });
    // 600 of 1,200 cents came back = half the net.
    expect(h.ledger.availableMicro(USER)).toBe(5_000_000n);
    expect(h.ledger.charge('pi_1')).toMatchObject({ refundedGrossCents: 600n, refundedNetMicro: 5_000_000n });
    await handleStripeEvent(h.deps, event);
    expect(h.ledger.availableMicro(USER)).toBe(5_000_000n);
    expect(h.alarms.join('\n')).toMatch(/outside this service/i);
  });

  it('a second dashboard refund debits only the new remainder, and a full refund empties the net exactly', async () => {
    const h = await withPurchase();
    await handleStripeEvent(h.deps, chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 401 }));
    // floor(10,000,000 x 401 / 1200) = 3,341,666
    expect(h.ledger.availableMicro(USER)).toBe(10_000_000n - 3_341_666n);
    await handleStripeEvent(h.deps, chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 1200 }));
    expect(h.ledger.availableMicro(USER)).toBe(0n);
    expect(h.ledger.charge('pi_1')).toMatchObject({ refundedGrossCents: 1200n, refundedNetMicro: 10_000_000n });
  });

  it('a dashboard refund after the credits were spent takes the balance below zero and alarms', async () => {
    const h = await withPurchase();
    const gate = makeGatekeeper({
      allowedHosts: [HOST],
      maxDepositPerSessionMicro: 100_000_000n,
      maxDailySpendMicro: 100_000_000n,
      maxOpensPerMinute: 100,
    });
    const hold = await h.ledger.openHold({ userId: USER, host: HOST, depositMicro: 8_000_000n }, gate);
    expect(hold.ok).toBe(true);
    await handleStripeEvent(h.deps, chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 1200 }));
    expect(h.ledger.availableMicro(USER)).toBe(-8_000_000n);
    expect(h.alarms.join('\n')).toMatch(/below zero/i);
  });

  it('the refund of a refused purchase is expected: no debit and no alarm', async () => {
    const h = await harness();
    await handleStripeEvent(h.deps, checkoutCompleted({ country: 'US', taxCents: 0, paymentIntent: 'pi_gate' }));
    h.alarms.length = 0;
    const res = await handleStripeEvent(
      h.deps,
      chargeRefunded({ paymentIntent: 'pi_gate', amountCents: 1000, amountRefundedCents: 1000 })
    );
    expect(res.status).toBe(200);
    expect(h.ledger.availableMicro(USER)).toBe(0n);
    expect(h.alarms).toEqual([]);
  });

  it("another product's charge on the shared account is ignored; an unknown charge of ours alarms", async () => {
    const h = await harness();
    const other = await handleStripeEvent(
      h.deps,
      chargeRefunded({ paymentIntent: 'pi_v2', amountCents: 500, amountRefundedCents: 500, metadata: { app: 'fabstir-v2' } })
    );
    expect(other.body).toMatchObject({ received: true, ignored: expect.any(String) });
    expect(h.alarms).toEqual([]);
    await handleStripeEvent(h.deps, chargeRefunded({ paymentIntent: 'pi_lost', amountCents: 500, amountRefundedCents: 500 }));
    expect(h.alarms.join('\n')).toMatch(/pi_lost/);
  });
});

describe('TX1.2 charge.refund.updated: a refund that never reached the card', () => {
  it('a failed dashboard refund is credited back and alarms, once however often the event is replayed', async () => {
    const h = await withPurchase();
    // Two dashboard refunds of 300 cents each: 600 of 1,200 cents back.
    await handleStripeEvent(h.deps, chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 600 }));
    expect(h.ledger.availableMicro(USER)).toBe(5_000_000n);
    h.alarms.length = 0;
    // One of them never reaches the card.
    const failed = refundUpdated({ eventId: 'evt_failed', refundId: 're_dash', paymentIntent: 'pi_1', amountCents: 300, status: 'failed' });
    await handleStripeEvent(h.deps, failed);
    expect(h.ledger.availableMicro(USER)).toBe(7_500_000n);
    expect(h.ledger.charge('pi_1')).toMatchObject({ refundedGrossCents: 300n, refundedNetMicro: 2_500_000n });
    expect(h.alarms.join('\n')).toMatch(/re_dash/);
    // Replayed (also under a new event id): credited back once.
    await handleStripeEvent(h.deps, failed);
    await handleStripeEvent(h.deps, { ...failed, id: 'evt_failed_again' });
    expect(h.ledger.availableMicro(USER)).toBe(7_500_000n);
    expect(h.ledger.charge('pi_1')).toMatchObject({ refundedGrossCents: 300n, refundedNetMicro: 2_500_000n });
  });

  it('a refund update that is not a failure changes nothing', async () => {
    const h = await withPurchase();
    await handleStripeEvent(h.deps, refundUpdated({ paymentIntent: 'pi_1', amountCents: 600, status: 'succeeded' }));
    expect(h.ledger.availableMicro(USER)).toBe(10_000_000n);
    expect(await h.journal()).toHaveLength(1);
  });

  it('a failed refund of a refused purchase alarms for a person: the buyer has neither credits nor money', async () => {
    const h = await harness();
    await handleStripeEvent(h.deps, checkoutCompleted({ country: 'US', taxCents: 0, paymentIntent: 'pi_gate' }));
    h.alarms.length = 0;
    await handleStripeEvent(
      h.deps,
      refundUpdated({ paymentIntent: 'pi_gate', amountCents: 1000, status: 'failed', metadata: { fc1Origin: 'gate' } })
    );
    expect(h.ledger.availableMicro(USER)).toBe(0n);
    expect(h.alarms.join('\n')).toMatch(/pi_gate/);
  });
});

describe('TX1.2 disputes (chargebacks)', () => {
  const gate = makeGatekeeper({
    allowedHosts: [HOST],
    maxDepositPerSessionMicro: 100_000_000n,
    maxDailySpendMicro: 100_000_000n,
    maxOpensPerMinute: 100,
  });

  it('a dispute debits its net share under the dispute id and freezes the user; won restores exactly and unfreezes', async () => {
    const h = await withPurchase(); // 1,000 credits, 1,200 cents
    const created = disputeEvent('charge.dispute.created', { eventId: 'evt_dc', disputeId: 'dp_1', paymentIntent: 'pi_1', amountCents: 1200 });
    await handleStripeEvent(h.deps, created);
    expect(h.ledger.availableMicro(USER)).toBe(0n);
    expect(h.ledger.isFrozen(USER)).toBe(true);
    expect(h.ledger.dispute('dp_1')).toMatchObject({ debitMicro: 10_000_000n, grossCents: 1200n, state: 'open' });
    expect(h.alarms.join('\n')).toMatch(/dp_1/);
    // A frozen user opens no session, whatever their balance.
    expect(await h.ledger.previewOpen({ userId: USER, host: HOST, depositMicro: 1n }, gate)).toEqual({
      allow: false,
      reason: 'ACCOUNT_FROZEN',
    });
    // Replay: one debit.
    await handleStripeEvent(h.deps, created);
    expect(h.ledger.availableMicro(USER)).toBe(0n);

    await handleStripeEvent(
      h.deps,
      disputeEvent('charge.dispute.closed', { disputeId: 'dp_1', paymentIntent: 'pi_1', amountCents: 1200, status: 'won' })
    );
    expect(h.ledger.availableMicro(USER)).toBe(10_000_000n);
    expect(h.ledger.isFrozen(USER)).toBe(false);
    expect(h.ledger.charge('pi_1')).toMatchObject({ remainingNetMicro: 10_000_000n, disputed: false });
  });

  it('a partial dispute after a refund debits only the disputed share, capped at what the charge still carries', async () => {
    const h = await withPurchase();
    // 300 credits already refunded (360 cents) from the dashboard.
    await handleStripeEvent(h.deps, chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 360 }));
    expect(h.ledger.availableMicro(USER)).toBe(7_000_000n);
    // A dispute for 600 cents = 500 credits.
    await handleStripeEvent(
      h.deps,
      disputeEvent('charge.dispute.created', { disputeId: 'dp_part', paymentIntent: 'pi_1', amountCents: 600 })
    );
    expect(h.ledger.dispute('dp_part')).toMatchObject({ debitMicro: 5_000_000n });
    expect(h.ledger.availableMicro(USER)).toBe(2_000_000n);

    // A dispute for the WHOLE charge after that refund debits only what is left.
    const h2 = await withPurchase();
    await handleStripeEvent(h2.deps, chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 360 }));
    await handleStripeEvent(
      h2.deps,
      disputeEvent('charge.dispute.created', { disputeId: 'dp_full', paymentIntent: 'pi_1', amountCents: 1200 })
    );
    expect(h2.ledger.dispute('dp_full')).toMatchObject({ debitMicro: 7_000_000n });
    expect(h2.ledger.availableMicro(USER)).toBe(0n);
  });

  it('an inquiry answered by refunding the charge: when it closes the customer has the money OR the credits, never both', async () => {
    const h = await withPurchase(); // 1,000 credits, 1,200 cents
    await handleStripeEvent(
      h.deps,
      disputeEvent('charge.dispute.created', { disputeId: 'dp_inq', paymentIntent: 'pi_1', amountCents: 1200, status: 'warning_needs_response' })
    );
    expect(h.ledger.availableMicro(USER)).toBe(0n);
    // The operator refunds the whole charge in the dashboard, as Stripe advises for an inquiry.
    await handleStripeEvent(h.deps, chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 1200 }));
    // Not debited twice while the dispute holds the same credits.
    expect(h.ledger.availableMicro(USER)).toBe(0n);
    // The inquiry closes without a chargeback: the dispute's debit is restored,
    // and the refund's debit, held back until now, takes its place.
    await handleStripeEvent(
      h.deps,
      disputeEvent('charge.dispute.closed', { disputeId: 'dp_inq', paymentIntent: 'pi_1', amountCents: 1200, status: 'warning_closed' })
    );
    expect(h.ledger.availableMicro(USER)).toBe(0n);
    expect(h.ledger.isFrozen(USER)).toBe(false);
    expect(h.ledger.charge('pi_1')).toMatchObject({ refundedGrossCents: 1200n, refundedNetMicro: 10_000_000n, remainingNetMicro: 0n });
    // And it replays to the same place.
    const reopened = await CreditsLedger.open(h.store);
    expect(reopened.availableMicro(USER)).toBe(0n);
    expect(reopened.charge('pi_1')).toEqual(h.ledger.charge('pi_1'));
  });

  it('a won dispute with only a PARTIAL refund made meanwhile debits just that refund', async () => {
    const h = await withPurchase();
    await handleStripeEvent(h.deps, disputeEvent('charge.dispute.created', { disputeId: 'dp_p', paymentIntent: 'pi_1', amountCents: 1200 }));
    await handleStripeEvent(h.deps, chargeRefunded({ paymentIntent: 'pi_1', amountCents: 1200, amountRefundedCents: 300 }));
    await handleStripeEvent(
      h.deps,
      disputeEvent('charge.dispute.closed', { disputeId: 'dp_p', paymentIntent: 'pi_1', amountCents: 1200, status: 'won' })
    );
    // 300 of 1,200 cents went back = 250 credits; the other 750 are the customer's again.
    expect(h.ledger.availableMicro(USER)).toBe(7_500_000n);
    expect(h.ledger.charge('pi_1')).toMatchObject({ refundedNetMicro: 2_500_000n, remainingNetMicro: 7_500_000n });
  });

  it('lost keeps the debit: spent credits leave the balance below zero, sessions stay blocked, the receivable is counted apart', async () => {
    const h = await withPurchase();
    const hold = await h.ledger.openHold({ userId: USER, host: HOST, depositMicro: 6_000_000n }, gate);
    expect(hold.ok).toBe(true);
    await handleStripeEvent(
      h.deps,
      disputeEvent('charge.dispute.created', { disputeId: 'dp_lost', paymentIntent: 'pi_1', amountCents: 1200 })
    );
    expect(h.ledger.availableMicro(USER)).toBe(-6_000_000n);
    h.alarms.length = 0;
    await handleStripeEvent(
      h.deps,
      disputeEvent('charge.dispute.closed', { disputeId: 'dp_lost', paymentIntent: 'pi_1', amountCents: 1200, status: 'lost' })
    );
    expect(h.ledger.availableMicro(USER)).toBe(-6_000_000n);
    expect(h.ledger.dispute('dp_lost')).toMatchObject({ state: 'lost' });
    // Not frozen any more, but a balance below zero opens nothing.
    expect(h.ledger.isFrozen(USER)).toBe(false);
    expect(await h.ledger.previewOpen({ userId: USER, host: HOST, depositMicro: 1n }, gate)).toEqual({
      allow: false,
      reason: 'INSUFFICIENT_BALANCE',
    });
    // The lost charge is never refundable again.
    expect(h.ledger.charge('pi_1')).toMatchObject({ remainingNetMicro: 0n, disputed: true });
    expect(h.alarms.join('\n')).toMatch(/below zero/i);

    // Solvency sums max(balance, 0): another user's credits are not reduced by this debt.
    await h.ledger.purchase('user-2', 3_000_000n, 'evt_other');
    expect(h.ledger.outstandingMicro()).toBe(3_000_000n + 6_000_000n); // user-2 + the live hold
    expect(h.ledger.receivableMicro()).toBe(6_000_000n);
  });

  it('a closed event that arrives before its created event is applied whole', async () => {
    const h = await withPurchase();
    await handleStripeEvent(
      h.deps,
      disputeEvent('charge.dispute.closed', { disputeId: 'dp_early', paymentIntent: 'pi_1', amountCents: 1200, status: 'lost' })
    );
    expect(h.ledger.availableMicro(USER)).toBe(0n);
    expect(h.ledger.dispute('dp_early')).toMatchObject({ state: 'lost', debitMicro: 10_000_000n });
    // The late created event changes nothing.
    await handleStripeEvent(
      h.deps,
      disputeEvent('charge.dispute.created', { disputeId: 'dp_early', paymentIntent: 'pi_1', amountCents: 1200 })
    );
    expect(h.ledger.availableMicro(USER)).toBe(0n);
    expect(h.ledger.isFrozen(USER)).toBe(false);
  });

  it('funds withdrawn and reinstated are journaled for reconciliation and never touch the balance', async () => {
    const h = await withPurchase();
    await handleStripeEvent(
      h.deps,
      disputeEvent('charge.dispute.created', { disputeId: 'dp_f', paymentIntent: 'pi_1', amountCents: 1200 })
    );
    const before = h.ledger.availableMicro(USER);
    await handleStripeEvent(
      h.deps,
      disputeEvent('charge.dispute.funds_withdrawn', { disputeId: 'dp_f', paymentIntent: 'pi_1', amountCents: 1200 })
    );
    await handleStripeEvent(
      h.deps,
      disputeEvent('charge.dispute.funds_reinstated', { disputeId: 'dp_f', paymentIntent: 'pi_1', amountCents: 1200 })
    );
    expect(h.ledger.availableMicro(USER)).toBe(before);
    expect(h.ledger.dispute('dp_f')).toMatchObject({ withdrawnCents: 1200n, reinstatedCents: 1200n });
    const types = (await h.journal()).map((l) => l.t);
    expect(types.filter((t) => t === 'dispute-funds')).toHaveLength(2);
  });

  it("a dispute on another product's charge is ignored", async () => {
    const h = await harness();
    const res = await handleStripeEvent(
      h.deps,
      disputeEvent('charge.dispute.created', { disputeId: 'dp_v2', paymentIntent: 'pi_v2', amountCents: 500 })
    );
    expect(res.body).toMatchObject({ received: true, ignored: expect.any(String) });
    expect(await h.journal()).toEqual([]);
  });
});

describe('TX1.2 the webhook route (signature, then the handler)', () => {
  const SECRET = 'whsec_test';
  let h: Awaited<ReturnType<typeof harness>>;

  beforeEach(async () => {
    process.env.FIAT_STRIPE_WEBHOOK_SECRET = SECRET;
    h = await harness();
    setFiatDepsForTest({ ledger: h.ledger });
    setStripeWebhookForTest({ stripe: h.deps.stripe, onAlarm: h.deps.onAlarm });
  });
  afterEach(() => {
    delete process.env.FIAT_STRIPE_WEBHOOK_SECRET;
    delete process.env.SALES_EU;
    setFiatDepsForTest(undefined);
    setStripeWebhookForTest(undefined);
  });

  function post(event: unknown) {
    const payload = JSON.stringify(event);
    return webhookRoute(
      new Request('http://fiat/v1/fiat/stripe/webhook', {
        method: 'POST',
        headers: { 'stripe-signature': stripeSignatureHeader(payload, SECRET, Math.floor(Date.now() / 1000)) },
        body: payload,
      })
    );
  }

  it('credits a UK purchase net of VAT through the signed route', async () => {
    const res = await post(checkoutCompleted({ credits: 500, taxCents: 100 }));
    expect(res.status).toBe(200);
    expect(h.ledger.availableMicro(USER)).toBe(5_000_000n);
  });

  it('reads SALES_EU from the environment: an Irish buyer is refunded while it is off', async () => {
    const res = await post(checkoutCompleted({ country: 'IE', taxCents: 230, sessionId: 'cs_ie' }));
    expect(res.status).toBe(200);
    expect(h.ledger.availableMicro(USER)).toBe(0n);
    expect(h.refunds.map((r) => r.idempotencyKey)).toEqual(['gate:cs_ie']);

    process.env.SALES_EU = 'on';
    await post(checkoutCompleted({ country: 'IE', taxCents: 230 }));
    expect(h.ledger.availableMicro(USER)).toBe(10_000_000n);
  });
});
