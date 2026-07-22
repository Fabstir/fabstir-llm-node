// FC1 — POST /api/fiat/purchase + the fc1UserId identity scheme. The route
// creates a Stripe Checkout Session tagged with the buyer's address so the
// webhook credits the right ledger balance; it never charges anyone itself.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { POST } from '../app/v1/fiat/purchase/route';
import { fiatUserId } from '../src/lib/fiat-identity';

const ADDR = '0x1234567890abcDEF1234567890abcdef12345678';
const ADDR_LC = ADDR.toLowerCase();

describe('fiatUserId (the identity scheme)', () => {
  it('lowercases a valid address', () => {
    expect(fiatUserId(ADDR)).toBe(ADDR_LC);
    expect(fiatUserId(ADDR_LC)).toBe(ADDR_LC);
  });
  it('rejects non-addresses so a bad id can never split a balance', () => {
    expect(() => fiatUserId('user-1')).toThrow();
    expect(() => fiatUserId('0x123')).toThrow();
    expect(() => fiatUserId('')).toThrow();
    expect(() => fiatUserId(undefined as unknown as string)).toThrow();
  });
});

function post(body: unknown, url = 'http://site.example/api/fiat/purchase') {
  return POST(new Request(url, { method: 'POST', body: typeof body === 'string' ? body : JSON.stringify(body) }));
}

describe('POST /api/fiat/purchase', () => {
  beforeEach(() => {
    process.env.FIAT_STRIPE_SECRET_KEY = 'sk_test_x';
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    delete process.env.FIAT_STRIPE_SECRET_KEY;
    delete process.env.FIAT_MAX_PURCHASE_CREDITS;
    delete process.env.FIAT_PURCHASE_SUCCESS_URL;
  });

  const stubCheckout = () => {
    const fetchMock = vi.fn(async () =>
      new Response(JSON.stringify({ id: 'cs_1', url: 'https://checkout.stripe.com/c/cs_1' }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      })
    );
    vi.stubGlobal('fetch', fetchMock);
    return fetchMock;
  };

  it('creates a Checkout Session tagged with the buyer address and returns its URL', async () => {
    const fetchMock = stubCheckout();
    const res = await post({ clientAddress: ADDR, credits: 500 });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ url: 'https://checkout.stripe.com/c/cs_1' });

    const [url, init] = fetchMock.mock.calls[0]! as unknown as [string, RequestInit];
    expect(url).toBe('https://api.stripe.com/v1/checkout/sessions');
    const form = new URLSearchParams(init.body as string);
    expect(form.get('metadata[fc1UserId]')).toBe(ADDR_LC); // lowercased address
    expect(form.get('line_items[0][price_data][unit_amount]')).toBe('500'); // 500 credits = 500 cents
    expect(form.get('mode')).toBe('payment');
    expect(form.get('success_url')).toBe('http://site.example/account?purchase=success');
  });

  it('400s on a bad address or bad credits, without calling Stripe', async () => {
    const fetchMock = stubCheckout();
    expect((await post({ clientAddress: 'user-1', credits: 500 })).status).toBe(400);
    expect((await post({ clientAddress: ADDR, credits: 12.5 })).status).toBe(400);
    expect((await post({ clientAddress: ADDR, credits: 49 })).status).toBe(400); // below Stripe minimum
    expect((await post({ clientAddress: ADDR, credits: '500' })).status).toBe(400); // must be a number
    expect((await post('not json')).status).toBe(400);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('enforces the max purchase cap', async () => {
    process.env.FIAT_MAX_PURCHASE_CREDITS = '10000';
    const fetchMock = stubCheckout();
    expect((await post({ clientAddress: ADDR, credits: 10001 })).status).toBe(400);
    expect((await post({ clientAddress: ADDR, credits: 10000 })).status).toBe(200);
    void fetchMock;
  });

  it('503s when Stripe is not configured', async () => {
    delete process.env.FIAT_STRIPE_SECRET_KEY;
    const res = await post({ clientAddress: ADDR, credits: 500 });
    expect(res.status).toBe(503);
  });

  it('maps a Stripe error to 502', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ error: { message: 'nope' } }), { status: 400 }))
    );
    const res = await post({ clientAddress: ADDR, credits: 500 });
    expect(res.status).toBe(502);
  });
});
