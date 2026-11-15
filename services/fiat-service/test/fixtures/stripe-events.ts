// CONSTRUCTED FIXTURES. These Stripe events are built by hand from the shapes
// in Stripe's API reference (Checkout Session, Charge, Refund, Dispute and the
// Event envelope); they are NOT captures of real test-mode payloads. The TX1
// plan (§9) asks for real ones: the test-mode run (TX1.6) must capture a real
// payload of each type below and compare it field by field with what these
// builders produce, then replace them. Only the fields the service reads are
// asserted on; the rest are there so the objects look like Stripe's.

import { packPriceAt, PRICING_TAG } from '../../src/lib/card-pricing';
import { vatInMinor } from '../../src/lib/credit-price';

export const USER = '0x1234567890abcdef1234567890abcdef12345678';

let seq = 0;
function next(prefix: string): string {
  seq += 1;
  return `${prefix}_constructed_${seq}`;
}

function envelope(type: string, object: Record<string, unknown>, id?: string) {
  return {
    id: id ?? next('evt'),
    object: 'event',
    api_version: '2026-08-26.dahlia',
    created: 1_794_000_000,
    livemode: false,
    type,
    data: { object },
  };
}


/** The fixtures' rate: 1.33 US dollars per pound (served since `at: 0` in the
 *  tests' fx seam, helpers/fx-feed.ts ALWAYS_133). */
export const FX_133 = 1_330_000;
/** The envelope's and the session's `created`. */
export const CREATED_S = 1_794_000_000;

export interface CheckoutOptions {
  eventId?: string;
  type?: string;
  sessionId?: string;
  paymentIntent?: string;
  userId?: string;
  /** The pack bought (GP: 500, 1000, 2500 or 10000). */
  credits?: number;
  /** The VAT inside the price, in pence (default: vatInMinor of the price). */
  taxCents?: number;
  country?: string;
  taxIds?: Array<{ type: string; value: string }>;
  taxStatus?: string | null;
  email?: string | null;
  /** Override amount_subtotal (to make it disagree with the price). */
  subtotalCents?: number;
  /** Override amount_total (default: the stamped price). */
  totalCents?: number;
  consent?: { terms?: string; over18?: string; useNow?: string } | null;
  paymentStatus?: string;
  currency?: string;
  /** The stamps (GP2). `null` drops one; a string is written as is. */
  fxMicro?: number | string | null;
  priceMinor?: number | string | null;
  pricing?: string | null;
  created?: number | null;
  expiresAt?: number | null;
}

/** `checkout.session.completed` for a card payment (mode=payment), as created
 *  by this service (GP2): a `gbp1` pack session, VAT inside the price,
 *  automatic tax on, billing address collected, expiring after 1,860 s. */
export function checkoutCompleted(o: CheckoutOptions = {}) {
  const credits = o.credits ?? 1000;
  const fx = o.fxMicro === undefined ? FX_133 : o.fxMicro;
  const shownPrice = typeof fx === 'number' ? packPriceAt(credits, fx) : packPriceAt(credits, FX_133);
  const price = o.priceMinor === undefined ? shownPrice : o.priceMinor;
  const total = o.totalCents ?? (typeof price === 'number' ? price : shownPrice);
  const tax = o.taxCents ?? vatInMinor(total, 2000);
  const subtotal = o.subtotalCents ?? total;
  const created = o.created === undefined ? CREATED_S : o.created;
  const expires = o.expiresAt === undefined ? (created ?? CREATED_S) + 1860 : o.expiresAt;
  const consent =
    o.consent === null
      ? {}
      : {
          fc1Terms: o.consent?.terms ?? '2026-10-23.1',
          fc1Over18: o.consent?.over18 ?? '1',
          fc1UseNow: o.consent?.useNow ?? '1',
        };
  const stamps: Record<string, string> = {};
  if (fx !== null) stamps.fc1FxMicro = String(fx);
  if (price !== null) stamps.fc1PriceMinor = String(price);
  if (o.pricing !== null) stamps.fc1Pricing = o.pricing ?? PRICING_TAG;
  return envelope(
    o.type ?? 'checkout.session.completed',
    {
      id: o.sessionId ?? next('cs_test'),
      object: 'checkout.session',
      adaptive_pricing: { enabled: false },
      amount_subtotal: subtotal,
      amount_total: total,
      automatic_tax: { enabled: true, liability: { type: 'self' }, status: o.taxStatus === undefined ? 'complete' : o.taxStatus },
      billing_address_collection: 'required',
      ...(created !== null ? { created } : {}),
      currency: o.currency ?? 'gbp',
      customer: 'cus_constructed',
      customer_creation: 'always',
      customer_details: {
        address: {
          city: 'London',
          country: o.country === undefined ? 'GB' : o.country,
          line1: '1 Example Street',
          line2: null,
          postal_code: 'SW1A 1AA',
          state: null,
        },
        email: o.email === undefined ? 'buyer@example.com' : o.email,
        name: 'A Buyer',
        phone: null,
        tax_exempt: 'none',
        tax_ids: o.taxIds ?? [],
      },
      ...(expires !== null ? { expires_at: expires } : {}),
      invoice: 'in_constructed',
      livemode: false,
      metadata: { fc1UserId: o.userId ?? USER, fc1Credits: String(credits), ...stamps, ...consent },
      mode: 'payment',
      payment_intent: o.paymentIntent ?? next('pi'),
      payment_status: o.paymentStatus ?? 'paid',
      status: 'complete',
      total_details: { amount_discount: 0, amount_shipping: 0, amount_tax: tax },
    },
    o.eventId
  );
}

/** A US-dollar session as this service created before GP (VAT on top, no
 *  stamps): paid after the switch, it is refunded in full. */
export function checkoutCompletedUsd(o: { credits?: number; taxCents?: number; paymentIntent?: string; sessionId?: string; eventId?: string } = {}) {
  const credits = o.credits ?? 1000;
  const tax = o.taxCents ?? 200;
  const e = checkoutCompleted({
    credits: 1000,
    currency: 'usd',
    totalCents: credits + tax,
    subtotalCents: credits,
    taxCents: tax,
    fxMicro: null,
    priceMinor: null,
    pricing: null,
    ...(o.paymentIntent ? { paymentIntent: o.paymentIntent } : {}),
    ...(o.sessionId ? { sessionId: o.sessionId } : {}),
    ...(o.eventId ? { eventId: o.eventId } : {}),
  });
  (e.data.object.metadata as Record<string, string>).fc1Credits = String(credits);
  return e;
}

/** `charge.refunded`: the Charge, with Stripe's CUMULATIVE amount_refunded. */
export function chargeRefunded(o: {
  eventId?: string;
  paymentIntent: string;
  amountCents: number;
  amountRefundedCents: number;
  metadata?: Record<string, string>;
}) {
  return envelope(
    'charge.refunded',
    {
      id: next('ch'),
      object: 'charge',
      amount: o.amountCents,
      amount_captured: o.amountCents,
      amount_refunded: o.amountRefundedCents,
      currency: 'usd',
      disputed: false,
      metadata: o.metadata ?? { app: 'platformless-ai', fc1UserId: USER },
      paid: true,
      payment_intent: o.paymentIntent,
      payment_method_details: { card: { brand: 'visa', country: 'GB', last4: '0000' }, type: 'card' },
      refunded: o.amountRefundedCents >= o.amountCents,
      status: 'succeeded',
    },
    o.eventId
  );
}

/** `charge.refund.updated`: the Refund. */
export function refundUpdated(o: {
  eventId?: string;
  refundId?: string;
  paymentIntent: string;
  amountCents: number;
  status: string;
  metadata?: Record<string, string>;
}) {
  return envelope(
    'charge.refund.updated',
    {
      id: o.refundId ?? next('re'),
      object: 'refund',
      amount: o.amountCents,
      charge: 'ch_constructed',
      currency: 'usd',
      failure_reason: o.status === 'failed' ? 'lost_or_stolen_card' : null,
      metadata: o.metadata ?? {},
      payment_intent: o.paymentIntent,
      reason: null,
      status: o.status,
    },
    o.eventId
  );
}

/** A `charge.dispute.*` event: the Dispute. */
export function disputeEvent(
  type:
    | 'charge.dispute.created'
    | 'charge.dispute.closed'
    | 'charge.dispute.funds_withdrawn'
    | 'charge.dispute.funds_reinstated',
  o: { eventId?: string; disputeId: string; paymentIntent: string; amountCents: number; status?: string }
) {
  return envelope(
    type,
    {
      id: o.disputeId,
      object: 'dispute',
      amount: o.amountCents,
      balance_transactions: [],
      charge: 'ch_constructed',
      currency: 'usd',
      is_charge_refundable: false,
      metadata: {},
      payment_intent: o.paymentIntent,
      reason: 'fraudulent',
      status: o.status ?? 'needs_response',
    },
    o.eventId
  );
}
