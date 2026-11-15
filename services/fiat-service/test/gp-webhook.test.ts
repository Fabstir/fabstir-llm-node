// GP3 — crediting a pack paid in pounds: the cap, the currency and stamp gates,
// and R14 (a) (b) (c) against what the feed served. Each uncreditable payment
// is refunded in full by the existing gate path and raises an URGENT fx alert.
import { describe, expect, it } from 'vitest';
import { CreditsLedger, MemoryLedgerStore } from '../src/lib/ledger';
import { handleStripeEvent, type StripeWebhookDeps } from '../src/lib/stripe-webhook';
import { allowedBillingCountries } from '../src/lib/sales-countries';
import { fakeStripeApi } from './helpers/fake-stripe';
import { ALWAYS_133, servedFx } from './helpers/fx-feed';
import { CREATED_S as C, USER, checkoutCompleted, checkoutCompletedUsd } from './fixtures/stripe-events';
import type { ServedLine } from '../src/lib/fx-rule';

const micro = (credits: number) => BigInt(credits) * 10_000n;

async function harness(lines: ServedLine[] = ALWAYS_133, o: { noFx?: boolean } = {}) {
  const store = new MemoryLedgerStore();
  const ledger = await CreditsLedger.open(store, { now: () => C * 1000 + 60_000 });
  const stripe = fakeStripeApi();
  const fx = servedFx(lines);
  const alarms: string[] = [];
  const deps: StripeWebhookDeps = {
    ledger,
    stripe: stripe.api,
    onAlarm: (m) => alarms.push(m),
    allowedCountries: allowedBillingCountries({}),
    log: () => {},
    now: () => C * 1000 + 60_000,
    ...(o.noFx ? {} : { fx }),
  };
  const journal = async () => (await store.load()).map((l) => JSON.parse(l) as Record<string, unknown>);
  return { ledger, stripe, fx, alarms, deps, journal };
}

type H = Awaited<ReturnType<typeof harness>>;
const expectRefused = async (h: H, reason: string) => {
  expect(h.ledger.availableMicro(USER)).toBe(0n);
  expect(h.stripe.calls).toHaveLength(1);
  expect(h.stripe.calls[0]!.amountCents).toBeUndefined(); // the whole charge
  expect(h.stripe.calls[0]!.metadata).toMatchObject({ fc1Origin: 'gate', fc1Reason: reason });
  expect((await h.journal())[0]).toMatchObject({ t: 'purchase-refused', reason });
  expect(h.fx.urgents.join('\n')).toMatch(new RegExp(`uncreditable payment \\(${reason}\\)`));
};

describe('GP3 a pack paid in pounds', () => {
  it('500 credits for £5.10 with £0.85 VAT: the pack, no alarm, no page', async () => {
    const h = await harness();
    const res = await handleStripeEvent(h.deps, checkoutCompleted({ credits: 500, paymentIntent: 'pi_500' }));
    expect(res.body).toMatchObject({ applied: true });
    expect(h.ledger.availableMicro(USER)).toBe(micro(500));
    expect(h.ledger.charge('pi_500')).toMatchObject({ grossCents: 510n, taxCents: 85n, currency: 'gbp' });
    expect(h.alarms).toEqual([]);
    expect(h.fx.urgents).toEqual([]);
  });

  it('async_payment_succeeded is judged the same way', async () => {
    const h = await harness();
    await handleStripeEvent(h.deps, checkoutCompleted({ type: 'checkout.session.async_payment_succeeded', credits: 2500 }));
    expect(h.ledger.availableMicro(USER)).toBe(micro(2500));
  });

  it('the cap: the 1.40 overshoot (the 500 pack at £4.88 buys 501) credits 500', async () => {
    const h = await harness([{ at: 0, fxMicro: 1_400_000 }]);
    // Stripe's VAT a penny off the module's, so the exact path is not taken.
    await handleStripeEvent(h.deps, checkoutCompleted({ credits: 500, fxMicro: 1_400_000, taxCents: 79 }));
    expect(h.ledger.availableMicro(USER)).toBe(micro(500));
    expect(h.alarms.join('\n')).toMatch(/credited 500 credits/);
  });

  it('a short payment credits what it buys, with an alarm', async () => {
    const h = await harness();
    await handleStripeEvent(h.deps, checkoutCompleted({ credits: 1000, totalCents: 500, taxCents: 83 }));
    const got = h.ledger.availableMicro(USER);
    expect(got).toBeGreaterThan(0n);
    expect(got).toBeLessThan(micro(1000));
    expect(h.alarms.join('\n')).toMatch(/not the 985 shown/);
  });

  it('Stripe\'s VAT a penny from ours still credits the pack (A7)', async () => {
    const h = await harness();
    await handleStripeEvent(h.deps, checkoutCompleted({ credits: 1000, taxCents: 165 }));
    expect(h.ledger.availableMicro(USER)).toBe(micro(1000));
    expect(h.alarms).toEqual([]);
  });

  it('a US-dollar session is refunded in full, urgently', async () => {
    const h = await harness();
    await handleStripeEvent(h.deps, checkoutCompletedUsd({ paymentIntent: 'pi_usd' }));
    await expectRefused(h, 'currency');
    expect((await h.journal())[0]).toMatchObject({ currency: 'usd' });
  });

  it('a missing or malformed stamp is refunded in full, urgently (pricing)', async () => {
    for (const o of [{ pricing: null }, { pricing: 'gbp0' }, { fxMicro: null }, { fxMicro: '1.33' }, { priceMinor: null }, { credits: 750 }, { created: null }, { expiresAt: null }] as const) {
      const h = await harness();
      await handleStripeEvent(h.deps, checkoutCompleted(o));
      await expectRefused(h, 'pricing');
    }
  });

  it('a replay and a duplicate change nothing; the country and tax gates are unchanged', async () => {
    const h = await harness();
    const e = checkoutCompleted({ eventId: 'evt_1', sessionId: 'cs_1', paymentIntent: 'pi_1' });
    await handleStripeEvent(h.deps, e);
    await handleStripeEvent(h.deps, e);
    await handleStripeEvent(h.deps, checkoutCompleted({ eventId: 'evt_2', sessionId: 'cs_1', paymentIntent: 'pi_1' }));
    expect(h.ledger.availableMicro(USER)).toBe(micro(1000));
    const g = await harness();
    expect((await handleStripeEvent(g.deps, checkoutCompleted({ country: 'FR' }))).body).toMatchObject({ refused: 'country' });
    expect((await handleStripeEvent(g.deps, checkoutCompleted({ taxStatus: 'failed' }))).body).toMatchObject({ refused: 'tax_status' });
    expect(g.fx.urgents).toEqual([]);
  });

  it('no feed: answered 503 so Stripe redelivers; nothing refunded or credited', async () => {
    const h = await harness(ALWAYS_133, { noFx: true });
    const res = await handleStripeEvent(h.deps, checkoutCompleted({}));
    expect(res.status).toBe(503);
    expect(h.stripe.calls).toEqual([]);
    expect(await h.journal()).toEqual([]);
  });
});

describe('GP3 R14', () => {
  it('(a) one micro above the 1.40 ceiling is refused, even if served', async () => {
    const h = await harness([{ at: 0, fxMicro: 1_400_001 }]);
    await handleStripeEvent(h.deps, checkoutCompleted({ fxMicro: 1_400_001, credits: 500, priceMinor: 488 }));
    await expectRefused(h, 'rate_ceiling');
  });

  it('(b) equal to the window\'s highest is credited; one micro above it is refused', async () => {
    const ok = await harness([{ at: 0, fxMicro: 1_330_000 }]);
    await handleStripeEvent(ok.deps, checkoutCompleted({ fxMicro: 1_330_000 }));
    expect(ok.ledger.availableMicro(USER)).toBe(micro(1000));
    const above = await harness([{ at: 0, fxMicro: 1_329_999 }]);
    await handleStripeEvent(above.deps, checkoutCompleted({ fxMicro: 1_330_000 }));
    await expectRefused(above, 'rate_window');
  });

  // The stamped 1.33 was served for one second only, at X; 1.30 before and after.
  const blip = (x: number): ServedLine[] => [
    { at: 0, fxMicro: 1_300_000 },
    { at: x, fxMicro: 1_330_000 },
    { at: x + 1, fxMicro: 1_300_000 },
  ];
  for (const [label, x, allowed] of [
    ['created - 300 (allowed)', C - 300, true],
    ['created - 301 (outside)', C - 301, false],
    ['created + 60 (allowed)', C + 60, true],
    ['created + 61 (outside)', C + 61, false],
  ] as const) {
    it(`(b) served at ${label}`, async () => {
      const h = await harness(blip(x));
      await handleStripeEvent(h.deps, checkoutCompleted({ fxMicro: 1_330_000 }));
      if (allowed) expect(h.ledger.availableMicro(USER)).toBe(micro(1000));
      else await expectRefused(h, 'rate_window');
    });
  }

  it('(b) an empty window (nothing served, or a lost served.jsonl) is refused', async () => {
    for (const lines of [[], [{ at: C + 120, fxMicro: 1_330_000 }], [{ at: 0, fxMicro: null }]] as ServedLine[][]) {
      const h = await harness(lines);
      await handleStripeEvent(h.deps, checkoutCompleted({}));
      await expectRefused(h, 'rate_window');
    }
  });

  it('(c) a lifetime of 1,920 s is credited; 1,921 s, zero and negative are refused', async () => {
    const ok = await harness();
    await handleStripeEvent(ok.deps, checkoutCompleted({ expiresAt: C + 1920 }));
    expect(ok.ledger.availableMicro(USER)).toBe(micro(1000));
    for (const expiresAt of [C + 1921, C, C - 1, C + 86_400]) {
      const h = await harness();
      await handleStripeEvent(h.deps, checkoutCompleted({ expiresAt }));
      await expectRefused(h, 'session_lifetime');
    }
  });
});
