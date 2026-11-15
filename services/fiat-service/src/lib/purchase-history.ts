// What the account page lists under "Your purchases" (Terms section 2: "Each
// purchase is listed on your account page"). A pure read of the ledger for
// ONE account: its card payments, the refunds made from them, and the
// payments that could not be credited and were refunded automatically.
// No Stripe call: the ledger is the record. It holds no invoice or receipt
// link (Stripe emails the VAT invoice to the buyer), so none is returned and
// `invoiceLinks` says so.
//
// GP (A6): every row carries its `currency` ('usd' for anything journaled
// before GP). Amounts named `...Cents` are in that currency's minor units
// (pence for 'gbp'). A refund row's VAT is the charge's own share of the leg
// (`tax x legGross / gross`, rounded half up), and the leg that empties the
// charge takes the rest of its VAT (A13), so the rows' VAT sums to the charge's.
import type { CashoutLegSnapshot, ChargeSnapshot, CreditsLedger } from './ledger';

export interface PurchaseRow {
  /** The payment reference to quote to support (the PaymentIntent id). */
  reference: string;
  at: number;
  creditsMicro: string;
  netCents: number;
  taxCents: number;
  grossCents: number;
  /** What has gone back to the card from this payment so far, VAT included. */
  refundedGrossCents: number;
  disputed: boolean;
  currency: string;
}

export interface RefundRow {
  reference: string;
  at: number;
  creditsMicro: string;
  netCents: number;
  taxCents: number;
  grossCents: number;
  state: 'pending' | 'confirmed' | 'reversed';
  currency: string;
}

export interface AutomaticRefundRow {
  reference?: string;
  at: number;
  grossCents: number;
  taxCents: number;
  reason: string;
  /** Whether the money went back to the card. False: the payment could not
   *  be refunded automatically (it was alarmed for a person to deal with). */
  refunded: boolean;
  currency: string;
}

export interface PurchaseHistory {
  purchases: PurchaseRow[];
  refunds: RefundRow[];
  automaticRefunds: AutomaticRefundRow[];
  /** Whether rows carry a link to the Stripe invoice or receipt. They do not:
   *  the ledger does not hold one. */
  invoiceLinks: false;
}

const newestFirst = <T extends { at: number }>(rows: T[]): T[] => rows.sort((a, b) => b.at - a.at);

/** round-half-up(a x b / c), exact. */
const shareHalfUp = (a: bigint, b: bigint, c: bigint): bigint => (c <= 0n ? 0n : (2n * a * b + c) / (2n * c));

/** Each leg's VAT share (A13), by leg key, walking each charge's legs in the
 *  order they were journaled. A reversed leg returned nothing: its share is
 *  shown proportionally and does not count toward the charge's. */
export function legTaxShares(legs: readonly CashoutLegSnapshot[], charges: ReadonlyMap<string, ChargeSnapshot>): Map<string, bigint> {
  const out = new Map<string, bigint>();
  const cum = new Map<string, { gross: bigint; tax: bigint }>();
  for (const leg of legs) {
    const charge = charges.get(leg.paymentIntentId);
    if (!charge) {
      out.set(leg.key, 0n);
      continue;
    }
    const proportional = shareHalfUp(charge.taxCents, leg.grossCents, charge.grossCents);
    if (leg.state === 'reversed') {
      out.set(leg.key, proportional);
      continue;
    }
    const c = cum.get(leg.paymentIntentId) ?? { gross: 0n, tax: 0n };
    const gross = c.gross + leg.grossCents;
    // Cumulative, as the gross is: the VAT refunded so far is the charge's VAT
    // share of the gross refunded so far, so the rows can never sum past it.
    let share = gross >= charge.grossCents ? charge.taxCents - c.tax : shareHalfUp(charge.taxCents, gross, charge.grossCents) - c.tax;
    if (share < 0n) share = 0n;
    if (share > leg.grossCents) share = leg.grossCents;
    cum.set(leg.paymentIntentId, { gross, tax: c.tax + share });
    out.set(leg.key, share);
  }
  return out;
}

/** The history of `userId`, and of nobody else: every row is filtered by the
 *  account the credential was issued to. */
export function purchaseHistory(ledger: CreditsLedger, userId: string): PurchaseHistory {
  const purchases = ledger
    .allCharges()
    .filter((c) => c.userId === userId)
    .map((c): PurchaseRow => ({
      reference: c.paymentIntentId,
      at: c.atMs,
      creditsMicro: c.netMicro.toString(),
      netCents: Number(c.grossCents - c.taxCents),
      taxCents: Number(c.taxCents),
      grossCents: Number(c.grossCents),
      refundedGrossCents: Number(c.refundedGrossCents),
      disputed: c.disputed,
      currency: c.currency,
    }));
  const charges = new Map(ledger.allCharges().map((c) => [c.paymentIntentId, c]));
  const userLegs = ledger.allCashoutLegs().filter((l) => l.userId === userId);
  const shares = legTaxShares(userLegs, charges);
  const refunds = userLegs.map((l): RefundRow => {
    const taxCents = Number(shares.get(l.key) ?? 0n);
    const grossCents = Number(l.grossCents);
    return {
      reference: l.paymentIntentId,
      at: l.atMs,
      creditsMicro: l.amountMicro.toString(),
      netCents: grossCents - taxCents,
      taxCents,
      grossCents,
      state: l.state,
      currency: l.currency,
    };
  });
  const automaticRefunds = ledger
    .refusedPurchases()
    .filter((r) => r.userId === userId)
    .map((r): AutomaticRefundRow => ({
      ...(r.paymentIntentId ? { reference: r.paymentIntentId } : {}),
      at: r.atMs,
      grossCents: Number(r.grossCents),
      taxCents: Number(r.taxCents),
      reason: r.reason,
      refunded: r.refunded,
      currency: r.currency,
    }));
  return {
    purchases: newestFirst(purchases),
    refunds: newestFirst(refunds),
    automaticRefunds: newestFirst(automaticRefunds),
    invoiceLinks: false,
  };
}
