// FC1.4 — the Stripe seam, with node:crypto only (no stripe package). Two
// halves: webhook signature verification (Stripe's v1 scheme: HMAC-SHA256 over
// "<t>.<raw body>") and a minimal refunds client for cash-out. Server-only.
import { createHmac, timingSafeEqual } from 'node:crypto';

const DEFAULT_TOLERANCE_SEC = 300;

/** Verify a Stripe-Signature header against the raw request body. */
export function verifyStripeSignature(
  payload: string,
  header: string,
  secret: string,
  opts?: { nowMs?: number; toleranceSec?: number }
): boolean {
  if (!header) return false;
  const parts = header.split(',').map((p) => p.split('='));
  const t = Number(parts.find(([k]) => k === 't')?.[1]);
  const v1s = parts.filter(([k]) => k === 'v1').map(([, v]) => v ?? '');
  if (!Number.isFinite(t) || v1s.length === 0) return false;

  const nowS = Math.floor((opts?.nowMs ?? Date.now()) / 1000);
  if (Math.abs(nowS - t) > (opts?.toleranceSec ?? DEFAULT_TOLERANCE_SEC)) return false;

  const expected = createHmac('sha256', secret).update(`${t}.${payload}`).digest('hex');
  return v1s.some(
    (v) =>
      v.length === expected.length &&
      /^[0-9a-f]+$/.test(v) &&
      timingSafeEqual(Buffer.from(v, 'hex'), Buffer.from(expected, 'hex'))
  );
}

/** Build a valid header — for tests and the FC1.4 runbook's synthetic probe. */
export function stripeSignatureHeader(payload: string, secret: string, timestampS: number): string {
  const mac = createHmac('sha256', secret).update(`${timestampS}.${payload}`).digest('hex');
  return `t=${timestampS},v1=${mac}`;
}

export type ExtractedPurchase =
  | { kind: 'purchase'; eventId: string; userId: string; amountMicro: bigint; paymentIntentId?: string }
  | { kind: 'ignored'; reason: string };

/**
 * Map a Stripe event to a ledger purchase. Cents x 10,000 = micro-USDC (1
 * cent = 1 credit; TODO(Jules): any pricing margin is a product decision).
 * Anything unusable is 'ignored' with a reason — the webhook 200s so Stripe
 * does not retry forever, and the reason is logged for reconciliation.
 */
export function extractPurchase(event: unknown): ExtractedPurchase {
  const e = event as {
    id?: unknown;
    type?: unknown;
    data?: { object?: Record<string, unknown> };
  };
  if (typeof e?.id !== 'string') return { kind: 'ignored', reason: 'event has no id' };
  if (e.type !== 'checkout.session.completed') {
    return { kind: 'ignored', reason: `unhandled event type ${String(e.type)}` };
  }
  const obj = e.data?.object;
  const userId = (obj?.metadata as Record<string, unknown> | undefined)?.fc1UserId;
  if (typeof userId !== 'string' || userId.length === 0) {
    return { kind: 'ignored', reason: `event ${e.id} has no metadata.fc1UserId` };
  }
  const expectedCurrency = process.env.FIAT_STRIPE_CURRENCY ?? 'usd';
  if (obj?.currency !== expectedCurrency) {
    return { kind: 'ignored', reason: `event ${e.id} currency ${String(obj?.currency)} != ${expectedCurrency}` };
  }
  const cents = obj?.amount_total;
  if (typeof cents !== 'number' || !Number.isInteger(cents) || cents <= 0) {
    return { kind: 'ignored', reason: `event ${e.id} has no positive integer amount_total` };
  }
  const paymentIntent = obj?.payment_intent;
  return {
    kind: 'purchase',
    eventId: e.id,
    userId,
    amountMicro: BigInt(cents) * 10_000n,
    ...(typeof paymentIntent === 'string' && paymentIntent ? { paymentIntentId: paymentIntent } : {}),
  };
}

export interface StripeRefunds {
  createRefund(paymentIntentId: string, amountCents: number): Promise<{ id: string }>;
}

export interface StripeCheckout {
  /** Create a hosted Checkout Session to buy credits. `fc1UserId` rides in the
   *  session metadata so the webhook (checkout.session.completed) credits the
   *  right ledger balance; the amount is in cents (1 cent = 1 credit). */
  createCheckoutSession(params: {
    fc1UserId: string;
    amountCents: number;
    successUrl: string;
    cancelUrl: string;
  }): Promise<{ id: string; url: string }>;
}

/** Minimal Checkout client over Stripe's form-encoded REST API. */
export function makeStripeCheckoutClient(): StripeCheckout {
  const key = process.env.FIAT_STRIPE_SECRET_KEY;
  if (!key) {
    throw new Error('FIAT_STRIPE_SECRET_KEY is not set — the fiat backend is not configured');
  }
  const currency = process.env.FIAT_STRIPE_CURRENCY ?? 'usd';
  return {
    async createCheckoutSession(params): Promise<{ id: string; url: string }> {
      const res = await fetch('https://api.stripe.com/v1/checkout/sessions', {
        method: 'POST',
        headers: {
          authorization: `Bearer ${key}`,
          'content-type': 'application/x-www-form-urlencoded',
        },
        body: new URLSearchParams({
          mode: 'payment',
          'payment_method_types[]': 'card',
          'line_items[0][price_data][currency]': currency,
          'line_items[0][price_data][product_data][name]': 'Platformless AI credits',
          'line_items[0][price_data][unit_amount]': String(params.amountCents),
          'line_items[0][quantity]': '1',
          // Both places carry it: the webhook reads the SESSION metadata; setting
          // it on the PaymentIntent too keeps provenance if the flow is inspected.
          'metadata[fc1UserId]': params.fc1UserId,
          'payment_intent_data[metadata][fc1UserId]': params.fc1UserId,
          success_url: params.successUrl,
          cancel_url: params.cancelUrl,
        }),
      });
      const json = (await res.json()) as { id?: unknown; url?: unknown; error?: { message?: unknown } };
      if (!res.ok || typeof json.id !== 'string' || typeof json.url !== 'string') {
        throw new Error(`Stripe checkout failed (${res.status}): ${String(json.error?.message ?? 'unknown error')}`);
      }
      return { id: json.id, url: json.url };
    },
  };
}

/** Minimal refunds client over Stripe's form-encoded REST API. */
export function makeStripeRefundsClient(): StripeRefunds {
  const key = process.env.FIAT_STRIPE_SECRET_KEY;
  if (!key) {
    throw new Error('FIAT_STRIPE_SECRET_KEY is not set — the fiat backend is not configured');
  }
  return {
    async createRefund(paymentIntentId: string, amountCents: number): Promise<{ id: string }> {
      const res = await fetch('https://api.stripe.com/v1/refunds', {
        method: 'POST',
        headers: {
          authorization: `Bearer ${key}`,
          'content-type': 'application/x-www-form-urlencoded',
        },
        body: new URLSearchParams({ payment_intent: paymentIntentId, amount: String(amountCents) }),
      });
      const json = (await res.json()) as { id?: unknown; error?: { message?: unknown } };
      if (!res.ok || typeof json.id !== 'string') {
        throw new Error(`Stripe refund failed (${res.status}): ${String(json.error?.message ?? 'unknown error')}`);
      }
      return { id: json.id };
    },
  };
}
