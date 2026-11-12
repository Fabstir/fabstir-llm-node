// FC1.4 — the Stripe seam, with node:crypto only (no stripe package). Two
// halves: webhook signature verification (Stripe's v1 scheme: HMAC-SHA256 over
// "<t>.<raw body>") and a minimal refunds client for cash-out. Server-only.
import { createHmac, timingSafeEqual } from 'node:crypto';

const DEFAULT_TOLERANCE_SEC = 300;

/** The Stripe API version every request is pinned to, so the shapes this
 *  service builds and reads never move with the account's default. The webhook
 *  endpoint must be created with the SAME version (events are rendered in the
 *  endpoint's version, not the request's). Changing it is a release: re-read
 *  the Checkout, Refund, Charge and Dispute shapes used here first. */
export const STRIPE_API_VERSION = '2026-08-26.dahlia';

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

/** Why a PAID session cannot be credited. The buyer was charged, so the
 *  webhook refunds them (it must never just drop the event). */
export type Uncreditable = 'currency' | 'tax_amount' | 'no_user';

export type ExtractedPurchase =
  | { kind: 'purchase'; eventId: string; userId: string; amountMicro: bigint; paymentIntentId?: string }
  | { kind: 'ignored'; reason: string; uncreditable?: Uncreditable };

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
  // Two creditable events, not one. For async payment methods (Bacs, SEPA)
  // `completed` fires when checkout finishes — BEFORE the money moves — and
  // `async_payment_succeeded` fires when it actually settles. For cards only
  // `completed` fires, already paid. Stripe never delivers both with
  // payment_status 'paid' for one session, so the event-id dedup still gives
  // exactly-once.
  if (e.type !== 'checkout.session.completed' && e.type !== 'checkout.session.async_payment_succeeded') {
    return { kind: 'ignored', reason: `unhandled event type ${String(e.type)}` };
  }
  const obj = e.data?.object;
  // The gate that makes the two-event scheme safe: credit ONLY settled money.
  // Without it, `completed` with payment_status 'unpaid' (an async method that
  // may yet fail) minted spendable credits for a payment that did not exist.
  // Latent while Checkout was card-only; found in the 27 July v2-guide review.
  if (obj?.payment_status !== 'paid') {
    return {
      kind: 'ignored',
      reason: `event ${e.id} payment_status ${String(obj?.payment_status)} != paid — not settled; an async method credits on async_payment_succeeded`,
    };
  }
  const userId = (obj?.metadata as Record<string, unknown> | undefined)?.fc1UserId;
  if (typeof userId !== 'string' || userId.length === 0) {
    // Not ours on the shared account, unless the session carries our tag (the
    // webhook decides): then it is a paid session nobody can be credited for.
    return { kind: 'ignored', reason: `event ${e.id} has no metadata.fc1UserId`, uncreditable: 'no_user' };
  }
  const expectedCurrency = process.env.FIAT_STRIPE_CURRENCY ?? 'usd';
  if (obj?.currency !== expectedCurrency) {
    return {
      kind: 'ignored',
      reason: `event ${e.id} currency ${String(obj?.currency)} != ${expectedCurrency}`,
      uncreditable: 'currency',
    };
  }
  const cents = obj?.amount_total;
  if (typeof cents !== 'number' || !Number.isInteger(cents) || cents <= 0) {
    return { kind: 'ignored', reason: `event ${e.id} has no positive integer amount_total` };
  }
  // Credit NET of tax. `amount_total` includes VAT, and crediting it would hand
  // the customer the tax as spendable balance while HMRC takes the cash — the
  // backing invariant (every credit backed by USDC actually received) forces
  // net, regardless of whether prices are displayed inclusive or exclusive;
  // display only changes the purchase-page copy. NOT `amount_subtotal`, which
  // is before discounts too and would over-credit under a promo code. When no
  // tax is configured `total_details.amount_tax` is 0 or absent and this is a
  // no-op, so the fix is safe under every VAT-policy outcome.
  const tax = (obj?.total_details as { amount_tax?: unknown } | undefined)?.amount_tax ?? 0;
  if (typeof tax !== 'number' || !Number.isInteger(tax) || tax < 0 || tax >= cents) {
    // A malformed or implausible tax figure must not fall back to crediting the
    // gross — that silently reintroduces the bug it guards against.
    return {
      kind: 'ignored',
      reason: `event ${e.id} has unusable total_details.amount_tax ${String(tax)}`,
      uncreditable: 'tax_amount',
    };
  }
  const paymentIntent = obj?.payment_intent;
  return {
    kind: 'purchase',
    eventId: e.id,
    userId,
    amountMicro: BigInt(cents - tax) * 10_000n,
    ...(typeof paymentIntent === 'string' && paymentIntent ? { paymentIntentId: paymentIntent } : {}),
  };
}

/** What the buyer agreed to on the page before paying; it rides in the session
 *  metadata and the webhook journals it with the purchase (legal list 2.5). */
export interface PurchaseConsent {
  /** The version of the Terms the buyer accepted. */
  termsVersion: string;
  /** The buyer stated they are 18 or over (always true when consent is sent). */
  over18: true;
  /** "I want to use my credits straight away": the buyer's express request to
   *  start inside the 14-day cancellation period. The box starts unticked and
   *  MUST be ticked to buy (owner decision, 2026-11-12): a buyer who would
   *  rather wait the 14 days contacts support. */
  useNow: true;
}

/** FIAT_STRIPE_TOS_CONSENT=on: Stripe Checkout itself shows a required
 *  checkbox restating the three statements, so Stripe holds evidence bound to
 *  the payer. Off by default: Stripe refuses the session unless a Terms of
 *  Service URL is set in the account's public details. */
export function stripeTosConsent(): boolean {
  return process.env.FIAT_STRIPE_TOS_CONSENT === 'on';
}

/** The text beside Stripe's checkbox (at most 1,200 characters). The link to
 *  the Terms is the one in the Stripe dashboard's public details. */
export const STRIPE_TOS_MESSAGE =
  'I agree to the [Terms of Service](TERMS_OF_SERVICE_URL). I am 18 or over. I want to use my credits straight away. ' +
  'I understand that once a clip has been delivered I cannot cancel the payment for that clip. Unused credits stay refundable.';

export interface StripeCheckout {
  /** Create a hosted Checkout Session to buy credits. `fc1UserId` rides in the
   *  session metadata so the webhook (checkout.session.completed) credits the
   *  right ledger balance; the amount is in cents BEFORE tax (1 cent = 1
   *  credit). Stripe Tax adds VAT on top for the buyer's country (TX1 §2). */
  createCheckoutSession(params: {
    fc1UserId: string;
    amountCents: number;
    successUrl: string;
    cancelUrl: string;
    consent?: PurchaseConsent;
    customerEmail?: string;
  }): Promise<{ id: string; url: string }>;
}

/** The tax code Stripe Tax applies to credits: electronically supplied
 *  services by default (TX1 OQ-4 is with the accountant; a changed answer is
 *  an env edit, not a release). */
export function stripeTaxCode(): string {
  return process.env.FIAT_STRIPE_TAX_CODE || 'txcd_10000000';
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
      const form = new URLSearchParams({
        mode: 'payment',
        // Valid in the pinned version; the next one (2026-10-28.endive) lists
        // `allowed_payment_method_types` in its place: switch when the pin moves.
        'payment_method_types[]': 'card',
        'line_items[0][price_data][currency]': currency,
        'line_items[0][price_data][product_data][name]': 'Platformless AI credits',
        'line_items[0][price_data][unit_amount]': String(params.amountCents),
        'line_items[0][quantity]': '1',
        // TX1 §2 — VAT through Stripe Tax, calculated ON TOP of the credits'
        // price (OQ-1 a): the buyer gets exactly the credits they chose and
        // "one credit is one US cent before VAT" stays true in every country.
        'automatic_tax[enabled]': 'true',
        'line_items[0][price_data][tax_behavior]': 'exclusive',
        'line_items[0][price_data][product_data][tax_code]': stripeTaxCode(),
        // The billing address is the place-of-supply evidence the webhook's
        // country gate reads. A customer per purchase is what lets Checkout
        // collect a tax id in payment mode (one guest customer each, accepted).
        billing_address_collection: 'required',
        customer_creation: 'always',
        'tax_id_collection[enabled]': 'true',
        // A VAT invoice for every purchase (a Stripe receipt is not one); the
        // seller's VAT number comes from the account's invoice settings.
        'invoice_creation[enabled]': 'true',
        // A promotion code would make the amount paid differ from the credits.
        allow_promotion_codes: 'false',
        // Adaptive Pricing is on at account level (shared with Fabstir v2). A
        // buyer paying in pounds would be charged and never credited, because
        // the webhook credits `currency` only; so it is off for every session.
        'adaptive_pricing[enabled]': 'false',
        // Both places carry it: the webhook reads the SESSION metadata; setting
        // it on the PaymentIntent too keeps provenance if the flow is inspected.
        'metadata[fc1UserId]': params.fc1UserId,
        'payment_intent_data[metadata][fc1UserId]': params.fc1UserId,
        // Our tag on the SESSION too, so a paid session that lost its user id
        // is still recognised as ours and refunded, never silently dropped.
        'metadata[app]': 'platformless-ai',
        // What was bought, so the webhook can alarm when Stripe's subtotal
        // differs from it.
        'metadata[fc1Credits]': String(params.amountCents),
        // The Stripe account is shared with Fabstir v2 and Radar rules key on
        // ::app:: (TX1 plan §6a). It must ride on the PaymentIntent: Radar
        // reads the payment's metadata, not the Checkout Session's.
        'payment_intent_data[metadata][app]': 'platformless-ai',
        success_url: params.successUrl,
        cancel_url: params.cancelUrl,
      });
      if (params.consent) {
        form.set('metadata[fc1Terms]', params.consent.termsVersion);
        form.set('metadata[fc1Over18]', '1');
        form.set('metadata[fc1UseNow]', params.consent.useNow ? '1' : '0');
      }
      if (stripeTosConsent()) {
        form.set('consent_collection[terms_of_service]', 'required');
        form.set('custom_text[terms_of_service_acceptance][message]', STRIPE_TOS_MESSAGE);
      }
      if (params.customerEmail) form.set('customer_email', params.customerEmail);
      const res = await fetch('https://api.stripe.com/v1/checkout/sessions', {
        method: 'POST',
        headers: {
          authorization: `Bearer ${key}`,
          'content-type': 'application/x-www-form-urlencoded',
          'stripe-version': STRIPE_API_VERSION,
        },
        body: form,
      });
      const json = (await res.json()) as { id?: unknown; url?: unknown; error?: { message?: unknown } };
      if (!res.ok || typeof json.id !== 'string' || typeof json.url !== 'string') {
        throw new Error(`Stripe checkout failed (${res.status}): ${String(json.error?.message ?? 'unknown error')}`);
      }
      return { id: json.id, url: json.url };
    },
  };
}

// ---------------------------------------------------------------------------
// TX1 — the rest of the Stripe seam: refunds with an idempotency key and
// metadata, the card-country evidence read, the refund look-up that follows an
// ambiguous error, and the lists reconciliation reads. All over Stripe's
// documented form-encoded REST API; no stripe package.
// ---------------------------------------------------------------------------

/** A Stripe call that did not succeed. `definitive` = Stripe ANSWERED and
 *  refused, so nothing was done. Anything else (no answer, a timeout, a 5xx, a
 *  409 on the idempotency key, a 429) may or may not have been done: look the
 *  result up before compensating. */
export class StripeApiError extends Error {
  constructor(
    message: string,
    readonly httpStatus: number | undefined,
    readonly code: string | undefined
  ) {
    super(message);
    this.name = 'StripeApiError';
  }
  get definitive(): boolean {
    const s = this.httpStatus;
    return s !== undefined && s >= 400 && s < 500 && s !== 409 && s !== 429;
  }
}

/** Dispute statuses that mean the dispute is still running. */
export const OPEN_DISPUTE_STATUSES: readonly string[] = [
  'warning_needs_response',
  'warning_under_review',
  'needs_response',
  'under_review',
  'appeal_needs_response',
  'appeal_under_review',
];

/** How a CLOSED dispute leaves the ledger, from its status. ONE rule, used by
 *  the webhook and by the daily reconciliation, so they cannot disagree:
 *  `won` and `warning_closed` (an inquiry that never became a chargeback)
 *  restore the debit; everything else keeps it: `lost`, `prevented`, the older
 *  `charge_refunded`, and any closing status this service does not know. */
export function closedDisputeKeepsDebit(status: string): boolean {
  return status !== 'won' && status !== 'warning_closed';
}

/** The ledger state a dispute with this Stripe status should be in. */
export function disputeLedgerState(status: string): 'open' | 'won' | 'lost' {
  if (OPEN_DISPUTE_STATUSES.includes(status)) return 'open';
  return closedDisputeKeepsDebit(status) ? 'lost' : 'won';
}

/** Stripe's answer when a charge has nothing left to refund. On a retried
 *  gate refund this is success: the refund was made on an earlier attempt. */
export function isAlreadyRefunded(e: unknown): boolean {
  return e instanceof StripeApiError && e.code === 'charge_already_refunded';
}

export interface StripeRefundRequest {
  paymentIntentId: string;
  /** Absent = the whole remaining charge. */
  amountCents?: number;
  /** Sent as the `Idempotency-Key` header (Stripe keeps it for 24 h). */
  idempotencyKey: string;
  metadata: Record<string, string>;
}

export interface StripeApi {
  createRefund(request: StripeRefundRequest): Promise<{ id: string; status?: string }>;
  /** The country the card was issued in, from the PaymentIntent's charge.
   *  `timeoutMs` bounds the read (the webhook must answer Stripe quickly). */
  cardCountry(paymentIntentId: string, opts?: { timeoutMs?: number }): Promise<string | undefined>;
  /** The payment as Stripe has it NOW: whose it is (the tags this service
   *  puts on every PaymentIntent), when it was made, and what has been
   *  refunded on its charge. */
  paymentState(paymentIntentId: string): Promise<StripePaymentState>;
  /** The refund on this PaymentIntent that carries `metadata[fc1Key] = key`. */
  findRefund(paymentIntentId: string, key: string): Promise<{ id: string; status?: string } | undefined>;
}

export interface StripePaymentState {
  /** `metadata.app` and `metadata.fc1UserId` of the PaymentIntent. */
  app?: string;
  userId?: string;
  /** Unix seconds. */
  createdS?: number;
  /** The charge's current cumulative `amount_refunded`, in cents. */
  amountRefundedCents?: number;
}

const STRIPE_TIMEOUT_MS = 20_000;

function stripeSecretKey(): string {
  const key = process.env.FIAT_STRIPE_SECRET_KEY;
  if (!key) {
    throw new Error('FIAT_STRIPE_SECRET_KEY is not set — the fiat backend is not configured');
  }
  return key;
}

async function stripeRequest(
  key: string,
  method: 'GET' | 'POST',
  path: string,
  opts: { form?: URLSearchParams; idempotencyKey?: string; timeoutMs?: number } = {}
): Promise<Record<string, unknown>> {
  let res: Response;
  try {
    res = await fetch(`https://api.stripe.com${path}`, {
      method,
      headers: {
        authorization: `Bearer ${key}`,
        'stripe-version': STRIPE_API_VERSION,
        ...(opts.form ? { 'content-type': 'application/x-www-form-urlencoded' } : {}),
        ...(opts.idempotencyKey ? { 'idempotency-key': opts.idempotencyKey } : {}),
      },
      ...(opts.form ? { body: opts.form } : {}),
      signal: AbortSignal.timeout(opts.timeoutMs ?? STRIPE_TIMEOUT_MS),
    });
  } catch (e) {
    // No answer: the request may or may not have reached Stripe.
    throw new StripeApiError(`Stripe ${method} ${path} got no answer: ${e instanceof Error ? e.message : String(e)}`, undefined, undefined);
  }
  let json: Record<string, unknown>;
  try {
    json = (await res.json()) as Record<string, unknown>;
  } catch {
    throw new StripeApiError(`Stripe ${method} ${path} answered ${res.status} with a body that is not JSON`, res.ok ? undefined : res.status, undefined);
  }
  if (!res.ok) {
    const error = json.error as { message?: unknown; code?: unknown } | undefined;
    throw new StripeApiError(
      `Stripe ${method} ${path} failed (${res.status}): ${String(error?.message ?? 'unknown error')}`,
      res.status,
      typeof error?.code === 'string' ? error.code : undefined
    );
  }
  return json;
}

export function makeStripeApi(): StripeApi {
  const key = stripeSecretKey();
  return {
    async createRefund(request) {
      const form = new URLSearchParams({ payment_intent: request.paymentIntentId });
      if (request.amountCents !== undefined) form.set('amount', String(request.amountCents));
      for (const [k, v] of Object.entries(request.metadata)) form.set(`metadata[${k}]`, v);
      const json = await stripeRequest(key, 'POST', '/v1/refunds', { form, idempotencyKey: request.idempotencyKey });
      if (typeof json.id !== 'string') {
        throw new StripeApiError('Stripe refund answered without an id', undefined, undefined);
      }
      return { id: json.id, ...(typeof json.status === 'string' ? { status: json.status } : {}) };
    },
    async paymentState(paymentIntentId) {
      const json = await stripeRequest(
        key,
        'GET',
        `/v1/payment_intents/${encodeURIComponent(paymentIntentId)}?expand[]=latest_charge`
      );
      const metadata = (json.metadata ?? {}) as Record<string, unknown>;
      const charge = json.latest_charge as { amount_refunded?: unknown } | string | null | undefined;
      const refunded = typeof charge === 'object' && charge ? charge.amount_refunded : undefined;
      return {
        ...(typeof metadata.app === 'string' && metadata.app ? { app: metadata.app } : {}),
        ...(typeof metadata.fc1UserId === 'string' && metadata.fc1UserId ? { userId: metadata.fc1UserId } : {}),
        ...(typeof json.created === 'number' ? { createdS: json.created } : {}),
        ...(typeof refunded === 'number' && Number.isInteger(refunded) && refunded >= 0
          ? { amountRefundedCents: refunded }
          : {}),
      };
    },
    async cardCountry(paymentIntentId, opts) {
      const json = await stripeRequest(
        key,
        'GET',
        `/v1/payment_intents/${encodeURIComponent(paymentIntentId)}?expand[]=latest_charge`,
        opts?.timeoutMs ? { timeoutMs: opts.timeoutMs } : {}
      );
      const charge = json.latest_charge as
        | { payment_method_details?: { card?: { country?: unknown } } }
        | string
        | null
        | undefined;
      const country = typeof charge === 'object' && charge ? charge.payment_method_details?.card?.country : undefined;
      return typeof country === 'string' && country ? country : undefined;
    },
    async findRefund(paymentIntentId, refundKey) {
      // Every page: "no refund under this key" is what lets a leg be reversed
      // or made again, so it must never be concluded from the first 100.
      const data = await stripeListAll(key, '/v1/refunds', `payment_intent=${encodeURIComponent(paymentIntentId)}`);
      const hit = data.find((r) => (r.metadata as Record<string, unknown> | undefined)?.fc1Key === refundKey);
      if (!hit || typeof hit.id !== 'string') return undefined;
      return { id: hit.id, ...(typeof hit.status === 'string' ? { status: hit.status } : {}) };
    },
  };
}

/** The env-built client, made on first use: work that needs no Stripe call
 *  (most webhook events, a reconciliation with nothing pending) runs even
 *  where the secret key is not set. */
export function lazyStripeApi(): StripeApi {
  let api: StripeApi | undefined;
  const get = () => (api ??= makeStripeApi());
  return {
    createRefund: (request) => get().createRefund(request),
    cardCountry: (paymentIntentId, opts) => get().cardCountry(paymentIntentId, opts),
    paymentState: (paymentIntentId) => get().paymentState(paymentIntentId),
    findRefund: (paymentIntentId, key) => get().findRefund(paymentIntentId, key),
  };
}

/** Walk a Stripe list endpoint (`limit=100`, `starting_after`) to its end. */
async function stripeListAll(key: string, path: string, query: string): Promise<Array<Record<string, unknown>>> {
  const out: Array<Record<string, unknown>> = [];
  let after: string | undefined;
  for (let page = 0; page < 500; page++) {
    const json = await stripeRequest(
      key,
      'GET',
      `${path}?limit=100&${query}${after ? `&starting_after=${encodeURIComponent(after)}` : ''}`
    );
    const data = Array.isArray(json.data) ? (json.data as Array<Record<string, unknown>>) : [];
    out.push(...data);
    const last = data[data.length - 1];
    if (json.has_more !== true || !last || typeof last.id !== 'string') return out;
    after = last.id;
  }
  throw new StripeApiError(`Stripe list ${path} did not end after 500 pages`, undefined, undefined);
}

const str = (v: unknown): string | undefined => (typeof v === 'string' && v ? v : undefined);

/** The lists the daily reconciliation reads (TX1 §8), mapped to the few
 *  fields it compares. Structural types: `fiat-reconcile.ts` owns the names. */
export function makeStripeReconcileSource() {
  const key = stripeSecretKey();
  return {
    async listPaidSessions(sinceS: number) {
      const rows = await stripeListAll(key, '/v1/checkout/sessions', `status=complete&created[gte]=${sinceS}`);
      return rows
        .filter((s) => s.payment_status === 'paid' && typeof s.id === 'string' && typeof s.amount_total === 'number')
        .map((s) => {
          const userId = str((s.metadata as Record<string, unknown> | undefined)?.fc1UserId);
          const pi = str(s.payment_intent);
          return {
            sessionId: s.id as string,
            ...(pi ? { paymentIntentId: pi } : {}),
            ...(userId ? { userId } : {}),
            grossCents: s.amount_total as number,
            taxCents: ((s.total_details as { amount_tax?: number } | undefined)?.amount_tax ?? 0) as number,
            currency: String(s.currency ?? ''),
            createdS: typeof s.created === 'number' ? s.created : 0,
          };
        });
    },
    async listRefunds(sinceS: number) {
      const rows = await stripeListAll(key, '/v1/refunds', `created[gte]=${sinceS}`);
      return rows
        .filter((r) => typeof r.id === 'string' && typeof r.amount === 'number')
        .map((r) => {
          const metadata = (r.metadata ?? {}) as Record<string, unknown>;
          const pi = str(r.payment_intent);
          const refundKey = str(metadata.fc1Key);
          const origin = str(metadata.fc1Origin);
          return {
            refundId: r.id as string,
            ...(pi ? { paymentIntentId: pi } : {}),
            amountCents: r.amount as number,
            status: String(r.status ?? ''),
            ...(refundKey ? { key: refundKey } : {}),
            ...(origin ? { origin } : {}),
          };
        });
    },
    async listDisputes(sinceS: number) {
      const rows = await stripeListAll(key, '/v1/disputes', `created[gte]=${sinceS}`);
      return rows
        .filter((d) => typeof d.id === 'string' && typeof d.amount === 'number')
        .map((d) => {
          const pi = str(d.payment_intent);
          return {
            disputeId: d.id as string,
            ...(pi ? { paymentIntentId: pi } : {}),
            amountCents: d.amount as number,
            status: String(d.status ?? ''),
          };
        });
    },
  };
}

/** What the webhook reads off a paid Checkout Session beyond the credit itself
 *  (TX1 §3). Every field is optional in the event; the handler decides. */
export interface CheckoutFacts {
  sessionId?: string;
  grossCents: number;
  taxCents: number;
  subtotalCents?: number;
  billingCountry?: string;
  taxIdPresent: boolean;
  taxStatus?: string;
  /** `metadata.fc1Credits`: what the buyer chose, in cents before tax. */
  creditsBought?: number;
  consent?: PurchaseConsentRecord;
  email?: string;
}

export interface PurchaseConsentRecord {
  termsVersion: string;
  over18: boolean;
  useNow: boolean;
}

/** Call only for an event `extractPurchase` mapped to a purchase. */
export function extractCheckoutFacts(event: unknown): CheckoutFacts {
  const obj = ((event as { data?: { object?: Record<string, unknown> } })?.data?.object ?? {}) as Record<string, unknown>;
  const metadata = (obj.metadata ?? {}) as Record<string, unknown>;
  const details = (obj.customer_details ?? {}) as {
    address?: { country?: unknown } | null;
    email?: unknown;
    tax_ids?: unknown;
  };
  const gross = obj.amount_total as number;
  const tax = ((obj.total_details as { amount_tax?: number } | undefined)?.amount_tax ?? 0) as number;
  const country = details.address?.country;
  const credits = typeof metadata.fc1Credits === 'string' && /^\d+$/.test(metadata.fc1Credits) ? Number(metadata.fc1Credits) : undefined;
  const taxStatus = (obj.automatic_tax as { status?: unknown } | undefined)?.status;
  return {
    ...(typeof obj.id === 'string' ? { sessionId: obj.id } : {}),
    grossCents: gross,
    taxCents: tax,
    ...(typeof obj.amount_subtotal === 'number' ? { subtotalCents: obj.amount_subtotal } : {}),
    ...(typeof country === 'string' && country ? { billingCountry: country.toUpperCase() } : {}),
    taxIdPresent: Array.isArray(details.tax_ids) && details.tax_ids.length > 0,
    ...(typeof taxStatus === 'string' ? { taxStatus } : {}),
    ...(credits !== undefined ? { creditsBought: credits } : {}),
    ...(typeof metadata.fc1Terms === 'string' && metadata.fc1Terms
      ? {
          consent: {
            termsVersion: metadata.fc1Terms,
            over18: metadata.fc1Over18 === '1',
            useNow: metadata.fc1UseNow === '1',
          },
        }
      : {}),
    ...(typeof details.email === 'string' && details.email ? { email: details.email } : {}),
  };
}
