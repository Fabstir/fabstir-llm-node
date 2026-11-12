// TX1.1 — the Checkout Session the purchase route creates: VAT through Stripe
// Tax (calculated on top of the credits' price), a VAT invoice for every
// purchase, the recorded consent, and the per-user rate limit. No network: the
// Stripe call is a stubbed fetch and the assertions read the form body.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { POST } from '../app/v1/fiat/purchase/route';
import { resetCheckoutRateLimitForTest } from '../src/lib/purchase-rate-limit';

const ADDR = '0x1234567890abcDEF1234567890abcdef12345678';
const OTHER = '0xabcdefabcdefabcdefabcdefabcdefabcdefabcd';
const CONSENT = { termsVersion: '2026-10-23.1', over18: true, useNow: true };

function post(body: unknown) {
  return POST(
    new Request('http://site.example/v1/fiat/purchase', { method: 'POST', body: JSON.stringify(body) })
  );
}

function stubCheckout() {
  const fetchMock = vi.fn(async () =>
    new Response(JSON.stringify({ id: 'cs_1', url: 'https://checkout.stripe.com/c/cs_1' }), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    })
  );
  vi.stubGlobal('fetch', fetchMock);
  return fetchMock;
}

function formOf(fetchMock: ReturnType<typeof stubCheckout>, call = 0): URLSearchParams {
  const [, init] = fetchMock.mock.calls[call]! as unknown as [string, RequestInit];
  return new URLSearchParams(init.body as string);
}

describe('TX1.1 Checkout Session body', () => {
  beforeEach(() => {
    process.env.FIAT_STRIPE_SECRET_KEY = 'sk_test_x';
    resetCheckoutRateLimitForTest();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    delete process.env.FIAT_STRIPE_SECRET_KEY;
    delete process.env.FIAT_STRIPE_TAX_CODE;
    delete process.env.FIAT_CONSENT_REQUIRED;
    delete process.env.FIAT_MAX_CHECKOUTS_PER_HOUR;
  });

  it('turns on automatic tax, exclusive of the price, with the digital services tax code', async () => {
    const fetchMock = stubCheckout();
    expect((await post({ clientAddress: ADDR, credits: 1000, consent: CONSENT })).status).toBe(200);
    const form = formOf(fetchMock);
    expect(form.get('automatic_tax[enabled]')).toBe('true');
    expect(form.get('line_items[0][price_data][tax_behavior]')).toBe('exclusive');
    expect(form.get('line_items[0][price_data][product_data][tax_code]')).toBe('txcd_10000000');
    // The price is the credits' price BEFORE tax: 1,000 credits = 1,000 cents.
    expect(form.get('line_items[0][price_data][unit_amount]')).toBe('1000');
    expect(form.get('line_items[0][price_data][currency]')).toBe('usd');
  });

  it('collects the billing address and a tax id, and creates a customer and an invoice', async () => {
    const fetchMock = stubCheckout();
    await post({ clientAddress: ADDR, credits: 500, consent: CONSENT });
    const form = formOf(fetchMock);
    expect(form.get('billing_address_collection')).toBe('required');
    expect(form.get('customer_creation')).toBe('always');
    expect(form.get('tax_id_collection[enabled]')).toBe('true');
    expect(form.get('invoice_creation[enabled]')).toBe('true');
  });

  it('allows no promotion codes, pins US dollars and records the credits bought', async () => {
    const fetchMock = stubCheckout();
    await post({ clientAddress: ADDR, credits: 2500, consent: CONSENT });
    const form = formOf(fetchMock);
    expect(form.get('allow_promotion_codes')).toBe('false');
    // Adaptive Pricing would let a buyer pay in pounds; the webhook credits US
    // dollars only, so it is switched off per session, whatever the account says.
    expect(form.get('adaptive_pricing[enabled]')).toBe('false');
    expect(form.get('metadata[fc1Credits]')).toBe('2500');
    expect(form.get('metadata[fc1UserId]')).toBe(ADDR.toLowerCase());
  });

  it('carries payment_intent_data[metadata][app]=platformless-ai (the Radar rules key on it)', async () => {
    const fetchMock = stubCheckout();
    await post({ clientAddress: ADDR, credits: 500, consent: CONSENT });
    expect(formOf(fetchMock).get('payment_intent_data[metadata][app]')).toBe('platformless-ai');
  });

  it('passes the customer email only when one is given', async () => {
    const fetchMock = stubCheckout();
    await post({ clientAddress: ADDR, credits: 500, consent: CONSENT });
    expect(formOf(fetchMock, 0).has('customer_email')).toBe(false);
    await post({ clientAddress: ADDR, credits: 500, consent: CONSENT, email: 'buyer@example.com' });
    expect(formOf(fetchMock, 1).get('customer_email')).toBe('buyer@example.com');
    expect((await post({ clientAddress: ADDR, credits: 500, consent: CONSENT, email: 'not an email' })).status).toBe(400);
  });

  it('takes the tax code from FIAT_STRIPE_TAX_CODE when the accountant changes it', async () => {
    process.env.FIAT_STRIPE_TAX_CODE = 'txcd_10103001';
    const fetchMock = stubCheckout();
    await post({ clientAddress: ADDR, credits: 500, consent: CONSENT });
    expect(formOf(fetchMock).get('line_items[0][price_data][product_data][tax_code]')).toBe('txcd_10103001');
  });
});

describe('TX1.1 consent recorded with the purchase', () => {
  beforeEach(() => {
    process.env.FIAT_STRIPE_SECRET_KEY = 'sk_test_x';
    resetCheckoutRateLimitForTest();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    delete process.env.FIAT_STRIPE_SECRET_KEY;
    delete process.env.FIAT_CONSENT_REQUIRED;
  });

  it('refuses a purchase without the Terms version or the 18 or over statement, before Stripe', async () => {
    const fetchMock = stubCheckout();
    const noConsent = await post({ clientAddress: ADDR, credits: 500 });
    expect(noConsent.status).toBe(400);
    expect(await noConsent.json()).toMatchObject({ error: 'consent_required' });
    expect((await post({ clientAddress: ADDR, credits: 500, consent: { termsVersion: '2026-10-23.1' } })).status).toBe(400);
    expect(
      (await post({ clientAddress: ADDR, credits: 500, consent: { termsVersion: '2026-10-23.1', over18: false } })).status
    ).toBe(400);
    expect((await post({ clientAddress: ADDR, credits: 500, consent: { over18: true } })).status).toBe(400);
    expect(
      (await post({ clientAddress: ADDR, credits: 500, consent: { termsVersion: 'bad version!', over18: true } })).status
    ).toBe(400);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('carries the Terms version, the age statement and the use straight away statement in the session metadata', async () => {
    const fetchMock = stubCheckout();
    await post({ clientAddress: ADDR, credits: 500, consent: CONSENT });
    const form = formOf(fetchMock, 0);
    expect(form.get('metadata[fc1Terms]')).toBe('2026-10-23.1');
    expect(form.get('metadata[fc1Over18]')).toBe('1');
    expect(form.get('metadata[fc1UseNow]')).toBe('1');
  });

  it('the "use my credits straight away" box MUST be ticked to buy, like the other two', async () => {
    const fetchMock = stubCheckout();
    for (const useNow of [false, undefined, 'yes', 1]) {
      const res = await post({ clientAddress: ADDR, credits: 500, consent: { termsVersion: '2026-10-23.1', over18: true, useNow } });
      expect(res.status).toBe(400);
      expect(await res.json()).toMatchObject({ error: 'consent_required' });
    }
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('FIAT_CONSENT_REQUIRED=off lets a client that does not send consent yet through, recording nothing', async () => {
    process.env.FIAT_CONSENT_REQUIRED = 'off';
    const fetchMock = stubCheckout();
    expect((await post({ clientAddress: ADDR, credits: 500 })).status).toBe(200);
    const form = formOf(fetchMock);
    expect(form.has('metadata[fc1Terms]')).toBe(false);
    expect(form.has('metadata[fc1Over18]')).toBe(false);
  });
});

describe('TX1.1 rate limit: 5 Checkout sessions per user per hour', () => {
  beforeEach(() => {
    process.env.FIAT_STRIPE_SECRET_KEY = 'sk_test_x';
    resetCheckoutRateLimitForTest();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
    delete process.env.FIAT_STRIPE_SECRET_KEY;
    delete process.env.FIAT_MAX_CHECKOUTS_PER_HOUR;
  });

  it('refuses the sixth session inside an hour with 429, per user, and frees the budget an hour later', async () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(new Date('2026-11-12T10:00:00Z'));
    const fetchMock = stubCheckout();
    for (let i = 0; i < 5; i++) {
      expect((await post({ clientAddress: ADDR, credits: 500, consent: CONSENT })).status).toBe(200);
    }
    const sixth = await post({ clientAddress: ADDR, credits: 500, consent: CONSENT });
    expect(sixth.status).toBe(429);
    expect(sixth.headers.get('retry-after')).toMatch(/^\d+$/);
    expect(fetchMock).toHaveBeenCalledTimes(5); // the refused one never reached Stripe

    // Another user is not affected.
    expect((await post({ clientAddress: OTHER, credits: 500, consent: CONSENT })).status).toBe(200);

    vi.setSystemTime(new Date('2026-11-12T11:00:01Z'));
    expect((await post({ clientAddress: ADDR, credits: 500, consent: CONSENT })).status).toBe(200);
  });

  it('a request refused before Stripe (bad amount, no consent) does not use the budget', async () => {
    stubCheckout();
    for (let i = 0; i < 8; i++) {
      expect((await post({ clientAddress: ADDR, credits: 1, consent: CONSENT })).status).toBe(400);
      expect((await post({ clientAddress: ADDR, credits: 500 })).status).toBe(400);
    }
    expect((await post({ clientAddress: ADDR, credits: 500, consent: CONSENT })).status).toBe(200);
  });
});
