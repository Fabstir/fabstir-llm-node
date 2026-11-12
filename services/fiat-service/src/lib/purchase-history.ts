// What the account page lists under "Your purchases" (Terms section 2: "Each
// purchase is listed on your account page"). A pure read of the ledger for
// ONE account: its card payments, the refunds made from them, and the
// payments that could not be credited and were refunded automatically.
// No Stripe call: the ledger is the record. It holds no invoice or receipt
// link (Stripe emails the VAT invoice to the buyer), so none is returned and
// `invoiceLinks` says so.
import type { CreditsLedger } from './ledger';

/** One credit is one US cent: 10,000 micro-units. */
const MICRO_PER_CENT = 10_000n;

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
}

export interface RefundRow {
  reference: string;
  at: number;
  creditsMicro: string;
  netCents: number;
  taxCents: number;
  grossCents: number;
  state: 'pending' | 'confirmed' | 'reversed';
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
    }));
  const refunds = ledger
    .allCashoutLegs()
    .filter((l) => l.userId === userId)
    .map((l): RefundRow => {
      const netCents = Number(l.amountMicro / MICRO_PER_CENT);
      const grossCents = Number(l.grossCents);
      return {
        reference: l.paymentIntentId,
        at: l.atMs,
        creditsMicro: l.amountMicro.toString(),
        netCents,
        taxCents: Math.max(0, grossCents - netCents),
        grossCents,
        state: l.state,
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
    }));
  return {
    purchases: newestFirst(purchases),
    refunds: newestFirst(refunds),
    automaticRefunds: newestFirst(automaticRefunds),
    invoiceLinks: false,
  };
}
