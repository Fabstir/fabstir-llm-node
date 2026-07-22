// FC1.4 — cash-out: the ONLY way fiat money leaves (Decision 6), as a Stripe
// refund to the original card. Debit-first (the ledger's serial queue makes
// the balance check race-safe), with an exact compensating reversal if Stripe
// then fails. The R3 refund window is a placeholder constant for the adviser.
import type { FiatCredentials } from './fiat-credentials';
import type { CreditsLedger } from './ledger';
import { makeStripeRefundsClient, type StripeRefunds } from './stripe';
import { getFiatDeps } from './fiat-session-service';

const DAY_MS = 86_400_000;
const MICRO_PER_CENT = 10_000n;

export interface CashoutDeps {
  ledger: CreditsLedger;
  credentials: FiatCredentials;
  stripe: StripeRefunds;
  /** TODO(Jules + adviser): the R3 policy number. Default 90 days. */
  refundWindowDays: number;
  now?: () => number;
}

export type CashoutRefusal =
  | 'INVALID_AMOUNT'
  | 'NOT_CENT_ALIGNED'
  | 'NO_REFUNDABLE_PURCHASE'
  | 'REFUND_WINDOW_EXPIRED'
  | 'EXCEEDS_REFUNDABLE'
  | 'INSUFFICIENT_BALANCE';

export type CashoutOutcome =
  | { status: 'ok'; refundId: string; amountMicro: bigint }
  | { status: 'unauthorised' }
  | { status: 'refused'; reason: CashoutRefusal }
  | { status: 'stripe_error'; message: string };

export async function requestCashout(
  deps: CashoutDeps,
  request: { credential: string; amountMicro: bigint }
): Promise<CashoutOutcome> {
  const userId = deps.credentials.authenticate(request.credential);
  if (!userId) return { status: 'unauthorised' };

  const amount = request.amountMicro;
  if (amount <= 0n) return { status: 'refused', reason: 'INVALID_AMOUNT' };
  if (amount % MICRO_PER_CENT !== 0n) return { status: 'refused', reason: 'NOT_CENT_ALIGNED' };

  const refundable = deps.ledger.latestRefundablePurchase(userId);
  if (!refundable) return { status: 'refused', reason: 'NO_REFUNDABLE_PURCHASE' };

  const nowMs = (deps.now ?? Date.now)();
  if (nowMs - refundable.atMs > deps.refundWindowDays * DAY_MS) {
    return { status: 'refused', reason: 'REFUND_WINDOW_EXPIRED' };
  }
  if (amount > refundable.remainingMicro) return { status: 'refused', reason: 'EXCEEDS_REFUNDABLE' };

  const debit = await deps.ledger.cashout(userId, amount, {
    paymentIntentId: refundable.paymentIntentId,
  });
  if (!debit.ok) return { status: 'refused', reason: debit.reason };

  try {
    const refund = await deps.stripe.createRefund(
      refundable.paymentIntentId,
      Number(amount / MICRO_PER_CENT)
    );
    return { status: 'ok', refundId: refund.id, amountMicro: amount };
  } catch (e) {
    await deps.ledger.reverseCashout(userId, amount, refundable.paymentIntentId);
    return { status: 'stripe_error', message: e instanceof Error ? e.message : String(e) };
  }
}

export interface CashoutService {
  request(request: { credential: string; amountMicro: bigint }): Promise<CashoutOutcome>;
}

let overrideService: CashoutService | undefined;
let builtService: CashoutService | undefined;

export function setCashoutServiceForTest(service: CashoutService | undefined): void {
  overrideService = service;
}

export async function getCashoutService(): Promise<CashoutService> {
  if (overrideService) return overrideService;
  if (!builtService) {
    const { ledger, credentials } = await getFiatDeps();
    const raw = process.env.FIAT_REFUND_WINDOW_DAYS;
    const refundWindowDays = raw === undefined || raw === '' ? 90 : Number(raw);
    if (!Number.isInteger(refundWindowDays) || refundWindowDays <= 0) {
      throw new Error(`FIAT_REFUND_WINDOW_DAYS must be a positive integer, got "${raw}"`);
    }
    const deps: CashoutDeps = { ledger, credentials, stripe: makeStripeRefundsClient(), refundWindowDays };
    builtService = { request: (request) => requestCashout(deps, request) };
  }
  return builtService;
}
