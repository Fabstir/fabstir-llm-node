// The page a customer returns to after Checkout depends on which product started the purchase: the service is shared
// by more than one site, so one global pair of URLs sends a Blender customer back to the web app. A caller whose
// Origin header has an entry in FIAT_PURCHASE_RETURN_BY_ORIGIN gets that entry; everyone else keeps the global pair.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { POST } from '../app/v1/fiat/purchase/route';
import { resetCheckoutRateLimitForTest } from '../src/lib/purchase-rate-limit';

const ADDR = '0x1234567890abcDEF1234567890abcdef12345678';
const CONSENT = { termsVersion: '2026-11-12.2', over18: true, useNow: true };
const BLENDER = 'https://timeline.platformlessai.org';
const MAP = JSON.stringify({ [BLENDER]: [`${BLENDER}/account?purchase=success`, `${BLENDER}/account?purchase=cancelled`] });

function post(origin?: string) {
  return POST(
    new Request('http://127.0.0.1:3020/v1/fiat/purchase', {
      method: 'POST',
      headers: origin ? { origin } : {},
      body: JSON.stringify({ clientAddress: ADDR, credits: 500, consent: CONSENT }),
    })
  );
}

function stub() {
  const f = vi.fn(async () =>
    new Response(JSON.stringify({ id: 'cs_1', url: 'https://checkout.stripe.com/c/cs_1' }), { status: 200, headers: { 'content-type': 'application/json' } })
  );
  vi.stubGlobal('fetch', f);
  return f;
}
const form = (f: ReturnType<typeof stub>) => new URLSearchParams((f.mock.calls[0]! as unknown as [string, RequestInit])[1].body as string);

describe('return URLs per calling site', () => {
  beforeEach(() => {
    process.env.FIAT_STRIPE_SECRET_KEY = 'sk_test_x';
    process.env.FIAT_PURCHASE_SUCCESS_URL = 'http://localhost:3022/settings?purchase=success';
    process.env.FIAT_PURCHASE_CANCEL_URL = 'http://localhost:3022/settings?purchase=cancel';
    resetCheckoutRateLimitForTest();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    for (const k of ['FIAT_STRIPE_SECRET_KEY', 'FIAT_PURCHASE_SUCCESS_URL', 'FIAT_PURCHASE_CANCEL_URL', 'FIAT_PURCHASE_RETURN_BY_ORIGIN']) delete process.env[k];
  });

  it('a caller whose origin is in the map returns to its own pages', async () => {
    process.env.FIAT_PURCHASE_RETURN_BY_ORIGIN = MAP;
    const f = stub();
    expect((await post(BLENDER)).status).toBe(200);
    expect(form(f).get('success_url')).toBe(`${BLENDER}/account?purchase=success`);
    expect(form(f).get('cancel_url')).toBe(`${BLENDER}/account?purchase=cancelled`);
  });

  it('any other origin, or none, keeps the global pair', async () => {
    process.env.FIAT_PURCHASE_RETURN_BY_ORIGIN = MAP;
    for (const origin of ['https://testnet.platformlessai.ai', `${BLENDER}.evil.example`, undefined]) {
      const f = stub();
      resetCheckoutRateLimitForTest();
      expect((await post(origin)).status).toBe(200);
      expect(form(f).get('success_url')).toBe('http://localhost:3022/settings?purchase=success');
      expect(form(f).get('cancel_url')).toBe('http://localhost:3022/settings?purchase=cancel');
      vi.unstubAllGlobals();
    }
  });

  it('a map that is not valid JSON, or an entry that is not two https or localhost URLs, is ignored', async () => {
    for (const bad of ['{not json', JSON.stringify({ [BLENDER]: ['javascript:alert(1)', 'https://x.example/'] }), JSON.stringify({ [BLENDER]: 'https://x.example/' })]) {
      process.env.FIAT_PURCHASE_RETURN_BY_ORIGIN = bad;
      const f = stub();
      resetCheckoutRateLimitForTest();
      expect((await post(BLENDER)).status).toBe(200);
      expect(form(f).get('success_url')).toBe('http://localhost:3022/settings?purchase=success');
      vi.unstubAllGlobals();
    }
  });
});
