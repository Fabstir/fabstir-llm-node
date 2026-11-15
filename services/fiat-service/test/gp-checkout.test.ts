// GP2 — the purchase route in pounds (packs only, R12, R15) and GET /packs.
// No network: Stripe is a stubbed fetch, the rate a pinned in-memory feed.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { POST } from '../app/v1/fiat/purchase/route';
import { GET } from '../app/v1/fiat/packs/route';
import { resetCheckoutRateLimitForTest } from '../src/lib/purchase-rate-limit';
import { setCardFxForTest } from '../src/lib/card-fx';
import { testFeed } from './helpers/fx-feed';

const ADDR = '0x1234567890abcDEF1234567890abcdef12345678';
const CONSENT = { termsVersion: '2026-11-12.2', over18: true, useNow: true };
const PACKS_133 = [
  { credits: 500, priceMinor: 510, vatMinor: 85 },
  { credits: 1000, priceMinor: 985, vatMinor: 164 },
  { credits: 2500, priceMinor: 2408, vatMinor: 401 },
  { credits: 10000, priceMinor: 9526, vatMinor: 1588 },
];

const post = (body: unknown) =>
  POST(new Request('http://svc.local/v1/fiat/purchase', { method: 'POST', body: JSON.stringify(body) }));

function stubStripe() {
  const f = vi.fn(async () =>
    new Response(JSON.stringify({ id: 'cs_1', url: 'https://checkout.stripe.com/c/cs_1' }), { status: 200 })
  );
  vi.stubGlobal('fetch', f);
  return f;
}
const formOf = (f: ReturnType<typeof stubStripe>) =>
  new URLSearchParams((f.mock.calls[0]! as unknown as [string, RequestInit])[1].body as string);

beforeEach(async () => {
  process.env.FIAT_STRIPE_SECRET_KEY = 'sk_test_x';
  resetCheckoutRateLimitForTest();
  setCardFxForTest((await testFeed({ pin: 1_330_000 })).feed);
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
  setCardFxForTest(undefined);
  delete process.env.FIAT_STRIPE_SECRET_KEY;
  delete process.env.FIAT_STRIPE_CURRENCY;
});

describe('POST /v1/fiat/purchase in pounds', () => {
  it('a credit amount that is not a pack: 400 not_a_pack, no session', async () => {
    const f = stubStripe();
    const res = await post({ clientAddress: ADDR, credits: 600, priceMinor: 600, consent: CONSENT });
    expect(res.status).toBe(400);
    expect(await res.json()).toMatchObject({ error: 'not_a_pack' });
    expect(f).not.toHaveBeenCalled();
  });

  it('no priceMinor: 400 price_required; a malformed one too', async () => {
    const f = stubStripe();
    for (const priceMinor of [undefined, null, '510', 5.1, 0, -510]) {
      const res = await post({ clientAddress: ADDR, credits: 500, priceMinor, consent: CONSENT });
      expect(res.status).toBe(400);
      expect(await res.json()).toMatchObject({ error: 'price_required' });
    }
    expect(f).not.toHaveBeenCalled();
  });

  it('a price that is not the price now: 409 price_changed with the current packs, no session (R12)', async () => {
    const f = stubStripe();
    const res = await post({ clientAddress: ADDR, credits: 500, priceMinor: 518, consent: CONSENT });
    expect(res.status).toBe(409);
    expect(await res.json()).toEqual({ error: 'price_changed', currency: 'gbp', pricing: 'gbp1', packs: PACKS_133 });
    expect(f).not.toHaveBeenCalled();
  });

  it('no rate served: 503 card_unavailable, no session', async () => {
    setCardFxForTest((await testFeed({ settings: { ok: false, problem: 'FIAT_CARD_FX_MAX_USD_PER_GBP must be 1.40' } })).feed);
    const f = stubStripe();
    const res = await post({ clientAddress: ADDR, credits: 500, priceMinor: 510, consent: CONSENT });
    expect(res.status).toBe(503);
    expect(await res.json()).toMatchObject({ error: 'card_unavailable' });
    expect(f).not.toHaveBeenCalled();
  });

  it('the Checkout form: gbp, the price VAT included, expires_at = now + 1,860, the four stamps on session and PaymentIntent', async () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(new Date('2026-11-14T10:00:00.700Z'));
    process.env.FIAT_STRIPE_CURRENCY = 'usd'; // retired: ignored
    const f = stubStripe();
    const res = await post({ clientAddress: ADDR, credits: 2500, priceMinor: 2408, consent: CONSENT });
    expect(res.status).toBe(200);
    const form = formOf(f);
    expect(form.get('line_items[0][price_data][currency]')).toBe('gbp');
    expect(form.get('line_items[0][price_data][unit_amount]')).toBe('2408');
    expect(form.get('line_items[0][price_data][tax_behavior]')).toBe('inclusive');
    expect(form.get('automatic_tax[enabled]')).toBe('true');
    expect(form.get('adaptive_pricing[enabled]')).toBe('false');
    expect(form.get('expires_at')).toBe(String(Math.floor(Date.parse('2026-11-14T10:00:00Z') / 1000) + 1860));
    for (const prefix of ['metadata', 'payment_intent_data[metadata]']) {
      expect(form.get(`${prefix}[fc1Credits]`)).toBe('2500');
      expect(form.get(`${prefix}[fc1FxMicro]`)).toBe('1330000');
      expect(form.get(`${prefix}[fc1PriceMinor]`)).toBe('2408');
      expect(form.get(`${prefix}[fc1Pricing]`)).toBe('gbp1');
    }
  });

  it('a refused price does not use the checkout budget', async () => {
    stubStripe();
    for (let i = 0; i < 8; i++) {
      expect((await post({ clientAddress: ADDR, credits: 500, priceMinor: 1, consent: CONSENT })).status).toBe(409);
    }
    expect((await post({ clientAddress: ADDR, credits: 500, priceMinor: 510, consent: CONSENT })).status).toBe(200);
  });
});

describe('GET /v1/fiat/packs', () => {
  it('answers the packs at the rate served, no-store, without the rate itself', async () => {
    const res = await GET();
    expect(res.status).toBe(200);
    expect(res.headers.get('cache-control')).toBe('no-store');
    const body = await res.json();
    expect(body).toEqual({ currency: 'gbp', pricing: 'gbp1', packs: PACKS_133 });
    expect(JSON.stringify(body)).not.toContain('1330000');
  });

  it('at another served rate the prices move (1.31)', async () => {
    setCardFxForTest((await testFeed({ pin: 1_310_000 })).feed);
    const body = await (await GET()).json();
    expect(body.packs.map((p: { priceMinor: number }) => p.priceMinor)).toEqual([518, 1000, 2446, 9672]);
  });

  it('no rate served: 503 card_unavailable, no-store', async () => {
    setCardFxForTest((await testFeed({ settings: { ok: false, problem: 'x' } })).feed);
    const res = await GET();
    expect(res.status).toBe(503);
    expect(res.headers.get('cache-control')).toBe('no-store');
    expect(await res.json()).toMatchObject({ error: 'card_unavailable' });
  });

  it('the rate behind the prices is in served.jsonl', async () => {
    const t = await testFeed({ pin: 1_320_000 });
    setCardFxForTest(t.feed);
    await GET();
    expect(t.files.data['served.jsonl']).toContain('"fxMicro":1320000');
  });
});
