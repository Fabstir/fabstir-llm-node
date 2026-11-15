// GP1 — card packs in pounds. The settings of SPEC-CARD-FX-RULE R13 as code
// constants, around v2's pricing module (`credit-price.ts`, copied unchanged
// and hash-pinned in test/gp-pricing.test.ts). There is no exchange-rate
// setting here: the rate is whatever the feed (`card-fx.ts`) serves at that
// instant, passed in as `fxMicro` (US dollars per pound x 10^6).
//
// A credit keeps its face value (one US cent, 10,000 micro-USDC); only the
// pence a pack costs moves with the rate.
import { cardNetMinor, creditsForMinor, packPriceMinor, vatInMinor } from './credit-price';
import { CEILING_MICRO, sessionLifetimeOk } from './fx-rule';

/** The packs on sale (D4): packs only, no free amount. */
export const PACK_CREDITS: readonly number[] = [500, 1000, 2500, 10000];
/** UK VAT, included in the price (R13). */
export const UK_VAT_BPS = 2000;
/** Card costs at the dearest card accepted, Stripe Tax's 0.5% included (R13). */
export const CARD_COST_BPS = 330;
/** The fixed card cost in pence, 28p in both products (D8). */
export const CARD_COST_FIXED_MINOR = 28;
/** The allowance for buying USDC with the pounds (R13). */
export const USDC_REFILL_BPS = 100;
/** Our 10% comes out of the host's payment, so none is priced in (D5). */
export const TREASURY_BPS = 0;
/** The `fc1Pricing` stamp of a session priced by this module. */
export const PRICING_TAG = 'gbp1';
/** The currency every `gbp1` session is charged in. */
export const CARD_CURRENCY = 'gbp';

export interface PackPrice {
  credits: number;
  /** The price in pence, VAT included. */
  priceMinor: number;
  /** The VAT inside it, for display (Stripe's own figure is what is charged). */
  vatMinor: number;
}

export const isPack = (credits: unknown): credits is number =>
  typeof credits === 'number' && PACK_CREDITS.includes(credits);

/** One pack's price at `fxMicro`. */
export function packPriceAt(credits: number, fxMicro: number): number {
  return packPriceMinor(credits, TREASURY_BPS, fxMicro, UK_VAT_BPS, CARD_COST_BPS, CARD_COST_FIXED_MINOR, USDC_REFILL_BPS);
}

/** Every pack's price and VAT at `fxMicro`. */
export function packsAt(fxMicro: number): PackPrice[] {
  return PACK_CREDITS.map((credits) => {
    const priceMinor = packPriceAt(credits, fxMicro);
    return { credits, priceMinor, vatMinor: vatInMinor(priceMinor, UK_VAT_BPS) };
  });
}

/** What `amountMinor` paid with `taxMinor` VAT in it buys at `fxMicro`, floored. */
export function creditsForPayment(amountMinor: number, taxMinor: number, fxMicro: number): number {
  const net = cardNetMinor(amountMinor, taxMinor, TREASURY_BPS, CARD_COST_BPS, CARD_COST_FIXED_MINOR, USDC_REFILL_BPS);
  return net > 0 ? creditsForMinor(net, TREASURY_BPS, fxMicro) : 0;
}

export interface CardCredits {
  /** Credits to add: never more than the pack. */
  credits: number;
  /** True when the payment is the pack's stamped price with its VAT (within
   *  Stripe's penny of rounding): the pack is credited and nothing is alarmed. */
  exact: boolean;
}

/**
 * The credits a paid `gbp1` session buys (plan 2.4 with A7). The pack is
 * credited when Stripe charged the stamped price and its VAT is within a penny
 * of `vatInMinor` (Stripe's rounding may differ by one). Anything else is
 * worked out from what was actually paid through v2's one net function, and
 * capped at the pack, so a payment never buys more than the pack shown (the
 * price search can overshoot by a credit, and a missing VAT would otherwise
 * hand the VAT out as credits).
 */
export function cardCreditsFor(p: {
  pack: number;
  fxMicro: number;
  priceMinor: number;
  amountTotal: number;
  amountTax: number;
}): CardCredits {
  if (p.amountTotal === p.priceMinor && Math.abs(p.amountTax - vatInMinor(p.priceMinor, UK_VAT_BPS)) <= 1) {
    return { credits: p.pack, exact: true };
  }
  const bought = creditsForPayment(p.amountTotal, p.amountTax, p.fxMicro);
  return { credits: Math.min(p.pack, bought), exact: false };
}

/** The pricing stamps of a `gbp1` session (GP2), as Stripe signed them. */
export interface CardStamp {
  /** The pack bought, in credits. */
  pack: number;
  /** US dollars per pound x 10^6 the price was worked out at. */
  fxMicro: number;
  /** The price shown and charged, in pence, VAT included. */
  priceMinor: number;
  /** Stripe's `created` and `expires_at`, unix seconds. */
  createdS: number;
  expiresAtS: number;
}

const wholeOf = (v: unknown): number | undefined =>
  typeof v === 'string' && /^[1-9][0-9]{0,15}$/.test(v) && Number.isSafeInteger(Number(v)) ? Number(v) : undefined;

/** A session's stamps when it is a `gbp1` pack session with every stamp well
 *  formed (whole numbers, no sign, no leading zero), else undefined. One reader
 *  for the webhook and the reconciliation. */
export function parseCardStamp(metadata: unknown, created: unknown, expires: unknown): CardStamp | undefined {
  const m = (metadata ?? {}) as Record<string, unknown>;
  if (m.fc1Pricing !== PRICING_TAG) return undefined;
  const pack = wholeOf(m.fc1Credits), fxMicro = wholeOf(m.fc1FxMicro), priceMinor = wholeOf(m.fc1PriceMinor);
  if (!isPack(pack) || fxMicro === undefined || priceMinor === undefined) return undefined;
  if (typeof created !== 'number' || !Number.isSafeInteger(created)) return undefined;
  if (typeof expires !== 'number' || !Number.isSafeInteger(expires)) return undefined;
  return { pack, fxMicro, priceMinor, createdS: created, expiresAtS: expires };
}

export type R14Refusal = 'rate_ceiling' | 'rate_window' | 'session_lifetime';

/**
 * SPEC R14 on a session's Stripe-signed stamps: (a) the stamped rate at or
 * below the 1.40 ceiling; (b) at or below the highest rate served from
 * `created − 300` to `created + 60` (`windowMax`; nothing served refuses);
 * (c) `expires_at − created` in (0, 1920]. Undefined when all three hold.
 * One function for the webhook and the reconciliation, so they cannot differ.
 */
export function r14Verdict(
  stamp: { fxMicro: number; createdS: number; expiresAtS: number },
  windowMax: (createdS: number) => number | undefined
): { reason: R14Refusal; why: string } | undefined {
  if (stamp.fxMicro > CEILING_MICRO) {
    return { reason: 'rate_ceiling', why: `stamped rate ${stamp.fxMicro} is above the ${CEILING_MICRO} ceiling (R14 a)` };
  }
  const max = windowMax(stamp.createdS);
  if (max === undefined || stamp.fxMicro > max) {
    return {
      reason: 'rate_window',
      why: `stamped rate ${stamp.fxMicro} is above the highest served from created-300 to created+60 (${max ?? 'none served'}) (R14 b)`,
    };
  }
  if (!sessionLifetimeOk(stamp.createdS, stamp.expiresAtS)) {
    return { reason: 'session_lifetime', why: `expires_at - created = ${stamp.expiresAtS - stamp.createdS} s, not in (0, 1920] (R14 c)` };
  }
  return undefined;
}
