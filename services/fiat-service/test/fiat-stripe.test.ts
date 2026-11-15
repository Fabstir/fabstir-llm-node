// FC1.4 — Stripe purchase: the ledger credits on the WEBHOOK only (Decision
// 6), verified with Stripe's v1 HMAC scheme using node:crypto (no stripe
// package), idempotent per event id. GP3: only a `gbp1` pack session is a
// purchase; extractPurchase reads its stamps, the webhook decides the credits.
import { createHmac } from 'node:crypto';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  extractPurchase,
  stripeSignatureHeader,
  verifyStripeSignature,
} from '../src/lib/stripe';
import { POST as webhook } from '../app/v1/fiat/stripe/webhook/route';
import { setFiatDepsForTest } from '../src/lib/fiat-session-service';
import { setStripeWebhookForTest } from '../src/lib/stripe-webhook';
import { fakeStripeApi } from './helpers/fake-stripe';
import { CreditsLedger, MemoryLedgerStore } from '../src/lib/ledger';
import { ALWAYS_133, servedFx } from './helpers/fx-feed';

const GBP1 = { fc1Credits: '500', fc1FxMicro: '1330000', fc1PriceMinor: '510', fc1Pricing: 'gbp1' };

const SECRET = 'whsec_test_secret';
const NOW_S = 1_700_000_000;

function sign(payload: string, t = NOW_S, secret = SECRET): string {
  const mac = createHmac('sha256', secret).update(`${t}.${payload}`).digest('hex');
  return `t=${t},v1=${mac}`;
}

describe('verifyStripeSignature (pure, node:crypto)', () => {
  const payload = '{"id":"evt_1"}';

  it('accepts a valid signature within tolerance', () => {
    expect(
      verifyStripeSignature(payload, sign(payload), SECRET, { nowMs: NOW_S * 1000 })
    ).toBe(true);
  });

  it('rejects a wrong secret, a tampered payload, and a malformed header', () => {
    expect(verifyStripeSignature(payload, sign(payload, NOW_S, 'whsec_other'), SECRET, { nowMs: NOW_S * 1000 })).toBe(false);
    expect(verifyStripeSignature('{"id":"evt_2"}', sign(payload), SECRET, { nowMs: NOW_S * 1000 })).toBe(false);
    expect(verifyStripeSignature(payload, 'nonsense', SECRET, { nowMs: NOW_S * 1000 })).toBe(false);
    expect(verifyStripeSignature(payload, '', SECRET, { nowMs: NOW_S * 1000 })).toBe(false);
  });

  it('rejects a stale timestamp (replay window) but accepts inside the default 300s', () => {
    expect(
      verifyStripeSignature(payload, sign(payload, NOW_S - 301), SECRET, { nowMs: NOW_S * 1000 })
    ).toBe(false);
    expect(
      verifyStripeSignature(payload, sign(payload, NOW_S - 299), SECRET, { nowMs: NOW_S * 1000 })
    ).toBe(true);
  });

  it('accepts when any one v1 entry matches (Stripe sends several during secret rolls)', () => {
    const good = sign(payload).split(',')[1];
    const header = `t=${NOW_S},v1=${'0'.repeat(64)},${good}`;
    expect(verifyStripeSignature(payload, header, SECRET, { nowMs: NOW_S * 1000 })).toBe(true);
  });
});

describe('extractPurchase', () => {
  const event = (over: Record<string, unknown> = {}, obj: Record<string, unknown> = {}) => ({
    id: 'evt_1',
    type: 'checkout.session.completed',
    data: {
      object: {
        metadata: { fc1UserId: 'user-1', ...GBP1 },
        amount_total: 510, // the 500 pack at 1.33: £5.10, VAT included
        total_details: { amount_tax: 85 },
        currency: 'gbp',
        created: 1_794_000_000,
        expires_at: 1_794_001_860,
        payment_status: 'paid', // real Stripe events always carry this
        payment_intent: 'pi_1',
        ...obj,
      },
    },
    ...over,
  });

  // The unpaid-credit bug (27 July guide review). For async payment methods
  // `completed` fires before the money moves, with payment_status 'unpaid'.
  // Crediting on the event alone minted credits for a payment that could still
  // fail. Latent while card-only, where completed implies paid.
  it('does NOT credit a completed checkout whose payment has not settled', () => {
    const out = extractPurchase(event({}, { payment_status: 'unpaid' }));
    expect(out.kind).toBe('ignored');
    if (out.kind === 'ignored') expect(out.reason).toContain('payment_status');
  });

  it('does not credit when payment_status is absent entirely', () => {
    const out = extractPurchase(event({}, { payment_status: undefined }));
    expect(out.kind).toBe('ignored');
  });

  it('credits async_payment_succeeded — the settle event for bank-debit methods', () => {
    const out = extractPurchase(event({ type: 'checkout.session.async_payment_succeeded' }));
    expect(out).toMatchObject({ kind: 'purchase', amountTotal: 510, amountTax: 85 });
  });

  it('still ignores async_payment_failed (no credit path for failure)', () => {
    expect(extractPurchase(event({ type: 'checkout.session.async_payment_failed' }))).toMatchObject({
      kind: 'ignored',
    });
  });

  // The VAT bug (26 July, fixed 27 July after the v2-guide review). amount_total
  // includes tax; crediting it gave the customer the VAT as spendable balance.
  // The backing invariant forces crediting NET, whatever the display policy.
  it('reads the VAT inside the price (the webhook credits from what was paid, never the gross as credits)', () => {
    const out = extractPurchase(event({}, { amount_total: 985, total_details: { amount_tax: 164 } }));
    expect(out).toMatchObject({ kind: 'purchase', amountTotal: 985, amountTax: 164 });
  });

  it('a zero or absent tax reads as 0 (the GB zero-VAT alarm and the cap handle it)', () => {
    expect(extractPurchase(event({}, { total_details: { amount_tax: 0 } }))).toMatchObject({ amountTax: 0 });
    expect(extractPurchase(event({}, { total_details: undefined }))).toMatchObject({ amountTax: 0 });
  });

  it('GP3: a session without the gbp1 stamps, or not a pack, is uncreditable (pricing)', () => {
    const bad: Array<Record<string, unknown>> = [
      { metadata: { fc1UserId: 'user-1', ...GBP1, fc1Pricing: undefined } },
      { metadata: { fc1UserId: 'user-1', ...GBP1, fc1Pricing: 'gbp2' } },
      { metadata: { fc1UserId: 'user-1', ...GBP1, fc1Credits: '600' } },
      { metadata: { fc1UserId: 'user-1', ...GBP1, fc1FxMicro: '1.33' } },
      { metadata: { fc1UserId: 'user-1', ...GBP1, fc1FxMicro: '-1330000' } },
      { metadata: { fc1UserId: 'user-1', ...GBP1, fc1FxMicro: '01330000' } },
      { metadata: { fc1UserId: 'user-1', ...GBP1, fc1PriceMinor: undefined } },
      { created: undefined },
      { expires_at: '1794001860' },
    ];
    for (const obj of bad) {
      expect(extractPurchase(event({}, obj)), JSON.stringify(obj)).toMatchObject({ kind: 'ignored', uncreditable: 'pricing' });
    }
  });

  it('GP3: a US-dollar session is uncreditable (currency), so it is refunded', () => {
    expect(extractPurchase(event({}, { currency: 'usd' }))).toMatchObject({ kind: 'ignored', uncreditable: 'currency' });
  });

  it('a malformed or implausible tax figure is ignored, never credited gross', () => {
    // Falling back to the gross on bad data would silently reintroduce the bug.
    for (const bad of [-1, 2.5, '200', 500, 600]) {
      expect(extractPurchase(event({}, { amount_total: 500, total_details: { amount_tax: bad } }))).toMatchObject(
        { kind: 'ignored' }
      );
    }
  });

  it('maps a completed gbp1 checkout to a purchase with its stamps', () => {
    expect(extractPurchase(event())).toEqual({
      kind: 'purchase',
      eventId: 'evt_1',
      userId: 'user-1',
      amountTotal: 510,
      amountTax: 85,
      currency: 'gbp',
      stamp: { pack: 500, fxMicro: 1_330_000, priceMinor: 510, createdS: 1_794_000_000, expiresAtS: 1_794_001_860 },
      paymentIntentId: 'pi_1',
    });
  });

  it('ignores other event types', () => {
    expect(extractPurchase(event({ type: 'invoice.paid' }))).toEqual({
      kind: 'ignored',
      reason: 'unhandled event type invoice.paid',
    });
  });

  it('ignores (with reasons) missing user metadata, wrong currency, and non-positive amounts', () => {
    expect(extractPurchase(event({}, { metadata: {} }))).toMatchObject({ kind: 'ignored' });
    expect(extractPurchase(event({}, { currency: 'eur' }))).toMatchObject({ kind: 'ignored' });
    expect(extractPurchase(event({}, { amount_total: 0 }))).toMatchObject({ kind: 'ignored' });
  });
});

describe('stripeSignatureHeader helper (used by tests and the FC1.4 runbook probe)', () => {
  it('produces a header verifyStripeSignature accepts', () => {
    const header = stripeSignatureHeader('{"a":1}', SECRET, NOW_S);
    expect(verifyStripeSignature('{"a":1}', header, SECRET, { nowMs: NOW_S * 1000 })).toBe(true);
  });
});

describe('POST /api/fiat/stripe/webhook', () => {
  let ledger: CreditsLedger;

  beforeEach(async () => {
    process.env.FIAT_STRIPE_WEBHOOK_SECRET = SECRET;
    ledger = await CreditsLedger.open(new MemoryLedgerStore());
    setFiatDepsForTest({ ledger });
    // No network: the card-country read answers GB; alarms are not under test here.
    setStripeWebhookForTest({ stripe: fakeStripeApi().api, onAlarm: () => {}, fx: servedFx(ALWAYS_133) });
  });
  afterEach(() => {
    delete process.env.FIAT_STRIPE_WEBHOOK_SECRET;
    setFiatDepsForTest(undefined);
    setStripeWebhookForTest(undefined);
  });

  const checkout = (eventId: string, _pack: 500) =>
    JSON.stringify({
      id: eventId,
      type: 'checkout.session.completed',
      data: {
        object: {
          id: `cs_${eventId}`,
          metadata: { fc1UserId: 'user-1', ...GBP1 },
          amount_total: 510,
          amount_subtotal: 510,
          payment_status: 'paid',
          currency: 'gbp',
          created: 1_794_000_000,
          expires_at: 1_794_001_860,
          payment_intent: 'pi_1',
          // TX1.2: a purchase is credited only when Stripe Tax completed and
          // the billing address is in a country we sell to.
          automatic_tax: { enabled: true, status: 'complete' },
          customer_details: { address: { country: 'GB' }, tax_ids: [] },
          total_details: { amount_tax: 85 },
        },
      },
    });

  // The route verifies against real time — sign with a live timestamp here.
  const signNow = (payload: string, secret = SECRET) =>
    sign(payload, Math.floor(Date.now() / 1000), secret);

  function post(payload: string, header?: string) {
    return webhook(
      new Request('http://site/api/fiat/stripe/webhook', {
        method: 'POST',
        headers: header === undefined ? {} : { 'stripe-signature': header },
        body: payload,
      })
    );
  }

  it('credits the ledger exactly once per event id (replay is a 200 no-op)', async () => {
    const payload = checkout('evt_10', 500);
    const first = await post(payload, signNow(payload));
    expect(first.status).toBe(200);
    expect(await first.json()).toEqual({ received: true, applied: true });
    expect(ledger.availableMicro('user-1')).toBe(5_000_000n);

    const replay = await post(payload, signNow(payload));
    expect(replay.status).toBe(200);
    expect(await replay.json()).toEqual({ received: true, applied: false });
    expect(ledger.availableMicro('user-1')).toBe(5_000_000n);
  });

  it('400s on a bad or missing signature and credits nothing', async () => {
    const payload = checkout('evt_11', 500);
    expect((await post(payload, signNow(payload, 'whsec_wrong'))).status).toBe(400);
    expect((await post(payload)).status).toBe(400);
    expect(ledger.availableMicro('user-1')).toBe(0n);
  });

  it('200s-and-ignores unhandled event types (Stripe must not retry them)', async () => {
    const payload = JSON.stringify({ id: 'evt_12', type: 'invoice.paid', data: { object: {} } });
    const res = await post(payload, signNow(payload));
    expect(res.status).toBe(200);
    expect(await res.json()).toMatchObject({ received: true, ignored: expect.any(String) });
  });

  it('503s when the webhook secret is not configured', async () => {
    delete process.env.FIAT_STRIPE_WEBHOOK_SECRET;
    const payload = checkout('evt_13', 500);
    expect((await post(payload, signNow(payload))).status).toBe(503);
  });
});
