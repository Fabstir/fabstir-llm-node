// FC1.4 — cash-out: the ONLY way fiat money leaves (Decision 6), as a Stripe
// refund to the original card. Debit-first (the ledger's serial queue makes
// the balance check race-safe), with an exact compensating reversal when
// Stripe did not make the refund.
//
// TX1.4 — "refund N credits" now returns the VAT paid on them too, and may
// span several card charges: the ledger walks the user's purchases newest
// first, inside the refund window, skipping disputed charges, and plans one
// LEG per charge (`planCashout`: every leg's debit is journaled as one line
// before Stripe is called). Each leg is one Stripe refund with
// `Idempotency-Key = cashout:<request id>:<payment intent>` and
// `metadata[fc1Origin] = cashout`. On an error that does not prove Stripe
// refused, the refund is LOOKED UP before compensating, so a timeout after a
// success can never leave the customer with both the cash and the credits.
import { randomUUID } from 'node:crypto';
import type { FiatCredentials } from './fiat-credentials';
import type { CreditsLedger, PlanCashoutReason } from './ledger';
import { makeStripeApi, StripeApiError, type StripeApi } from './stripe';
import { getFiatDeps } from './fiat-session-service';

const DAY_MS = 86_400_000;
const MICRO_PER_CENT = 10_000n;

export interface CashoutDeps {
  ledger: CreditsLedger;
  credentials: FiatCredentials;
  stripe: StripeApi;
  /** The refund window in days from each purchase (the Terms say 90). */
  refundWindowDays: number;
  now?: () => number;
  /** The request id inside every leg's idempotency key. Default: a UUID. */
  newRequestId?: () => string;
  onAlarm?: (message: string) => void;
}

export type CashoutRefusal = PlanCashoutReason | 'NOT_CENT_ALIGNED';

/** GP (A6): what goes back to the card, per currency (a refund can span an
 *  old US-dollar charge and a pound charge; `grossCents` sums both and means
 *  something only when there is one currency). */
export type ByCurrency = Array<{ currency: string; grossMinor: number }>;

/** The refund id a 0p leg is confirmed with (A1): no Stripe refund exists. */
export const ZERO_LEG_REFUND_ID = 'none:0p';

function byCurrencyOf(legs: ReadonlyArray<{ currency: string; grossCents: number }>): ByCurrency {
  const sums = new Map<string, number>();
  for (const l of legs) sums.set(l.currency, (sums.get(l.currency) ?? 0) + l.grossCents);
  return [...sums].map(([currency, grossMinor]) => ({ currency, grossMinor }));
}

export interface RefundedLeg {
  paymentIntentId: string;
  refundId: string;
  /** Credits (net, micro-units) this leg took. */
  amountMicro: bigint;
  /** What went back to the card for it, VAT included, in the minor units of `currency`. */
  grossCents: number;
  currency: string;
}

export interface FailedLeg {
  paymentIntentId: string;
  amountMicro: bigint;
  grossCents: number;
  currency: string;
  /** `reversed`: no refund was made and the credits are back. `unknown`: it
   *  could not be established whether Stripe made the refund, so the credits
   *  stay debited until a person or the next webhook settles it. */
  state: 'reversed' | 'unknown';
  message: string;
}

export type CashoutOutcome =
  | {
      status: 'ok';
      /** One entry per card charge refunded. */
      refunds: RefundedLeg[];
      /** Legs that did not go through (empty in the normal case). */
      failed: FailedLeg[];
      /** Total returned to the card, VAT included, in cents. */
      grossCents: number;
      /** Total credits refunded (net, micro-units). */
      amountMicro: bigint;
      byCurrency: ByCurrency;
    }
  | { status: 'unauthorised' }
  | { status: 'refused'; reason: CashoutRefusal }
  | { status: 'stripe_error'; message: string; failed: FailedLeg[] };

export type CashoutPreview =
  | {
      status: 'ok';
      grossCents: number;
      amountMicro: bigint;
      byCurrency: ByCurrency;
      legs: Array<{ paymentIntentId: string; amountMicro: bigint; grossCents: number; currency: string }>;
    }
  | { status: 'unauthorised' }
  | { status: 'refused'; reason: CashoutRefusal };

/**
 * After a leg is reversed because its refund was made and FAILED: bring the
 * ledger to the charge's current refunded figure. A refund made outside this
 * service may have been hidden behind that leg (its `charge.refunded` looked
 * explained by it), and nothing further would arrive to correct it. Never
 * throws: if Stripe cannot be read now, it is alarmed, and the next
 * `charge.refunded` for the charge, or the daily reconciliation, catches it.
 */
async function resyncAfterFailure(
  ledger: CreditsLedger,
  stripe: StripeApi,
  paymentIntentId: string,
  tag: string,
  onAlarm: (message: string) => void
): Promise<void> {
  try {
    const refundedNow = (await stripe.paymentState(paymentIntentId)).amountRefundedCents;
    if (refundedNow === undefined) return;
    const sync = await ledger.chargeRefunded(paymentIntentId, refundedNow, `resync:${tag}`);
    if (sync.known && !sync.refusedPurchase && sync.applied) {
      onAlarm(
        `after a failed refund on ${paymentIntentId}, Stripe shows ${sync.unexplainedCents} minor units refunded that the ledger had not debited: ` +
          `${sync.debitedMicro} micro debited from ${sync.userId}`
      );
    }
  } catch (e) {
    onAlarm(
      `${paymentIntentId}: the charge could not be read after a failed refund (${e instanceof Error ? e.message : String(e)}); ` +
        `the next charge.refunded or the daily reconciliation will compare it`
    );
  }
}

function validate(amount: bigint): CashoutRefusal | undefined {
  if (amount <= 0n) return 'INVALID_AMOUNT';
  if (amount % MICRO_PER_CENT !== 0n) return 'NOT_CENT_ALIGNED';
  return undefined;
}

/** What a refund of `amountMicro` would return, VAT included, before the
 *  customer confirms (TX1 §4). Changes nothing. */
export async function previewCashout(
  deps: CashoutDeps,
  request: { credential: string; amountMicro: bigint }
): Promise<CashoutPreview> {
  const userId = deps.credentials.authenticate(request.credential);
  if (!userId) return { status: 'unauthorised' };
  const invalid = validate(request.amountMicro);
  if (invalid) return { status: 'refused', reason: invalid };
  const plan = await deps.ledger.previewCashout(userId, request.amountMicro, {
    windowMs: deps.refundWindowDays * DAY_MS,
    nowMs: (deps.now ?? Date.now)(),
  });
  if (!plan.ok) return { status: 'refused', reason: plan.reason };
  const legs = plan.legs.map((l) => ({
    paymentIntentId: l.paymentIntentId,
    amountMicro: l.amountMicro,
    grossCents: Number(l.grossCents),
    currency: l.currency,
  }));
  return {
    status: 'ok',
    grossCents: Number(plan.grossCents),
    amountMicro: request.amountMicro,
    byCurrency: byCurrencyOf(legs),
    legs,
  };
}

export async function requestCashout(
  deps: CashoutDeps,
  request: { credential: string; amountMicro: bigint }
): Promise<CashoutOutcome> {
  const userId = deps.credentials.authenticate(request.credential);
  if (!userId) return { status: 'unauthorised' };

  const invalid = validate(request.amountMicro);
  if (invalid) return { status: 'refused', reason: invalid };

  const alarm = deps.onAlarm ?? ((m: string) => console.error(`[fiat-cashout] ALARM: ${m}`));
  const requestId = (deps.newRequestId ?? randomUUID)();
  // The debit: every leg, one journal line, under the ledger's mutex.
  const plan = await deps.ledger.planCashout(userId, request.amountMicro, {
    requestId,
    windowMs: deps.refundWindowDays * DAY_MS,
    nowMs: (deps.now ?? Date.now)(),
  });
  if (!plan.ok) return { status: 'refused', reason: plan.reason };

  const refunds: RefundedLeg[] = [];
  const failed: FailedLeg[] = [];
  for (const leg of plan.legs) {
    if (leg.grossCents === 0n) continue; // A1: handled after the legs that move money
    const summary = {
      paymentIntentId: leg.paymentIntentId,
      amountMicro: leg.amountMicro,
      grossCents: Number(leg.grossCents),
      currency: leg.currency,
    };
    let refundId: string | undefined;
    /** A refund that WAS made and came back failed or cancelled. */
    let failedRefundId: string | undefined;
    let failure: string | undefined;
    let ambiguous = false;
    try {
      const refund = await deps.stripe.createRefund({
        paymentIntentId: leg.paymentIntentId,
        amountCents: Number(leg.grossCents),
        idempotencyKey: leg.key,
        metadata: { fc1Origin: 'cashout', fc1Key: leg.key, fc1UserId: userId },
      });
      if (refund.status === 'failed' || refund.status === 'canceled') {
        failure = `Stripe created refund ${refund.id} with status ${refund.status}`;
        failedRefundId = refund.id;
      } else {
        refundId = refund.id;
      }
    } catch (e) {
      failure = e instanceof Error ? e.message : String(e);
      ambiguous = !(e instanceof StripeApiError && e.definitive);
    }

    if (refundId === undefined && ambiguous) {
      // Stripe did not say no. Look the refund up before giving credits back.
      try {
        const found = await deps.stripe.findRefund(leg.paymentIntentId, leg.key);
        if (found && found.status !== 'failed' && found.status !== 'canceled') refundId = found.id;
        else if (found) failedRefundId = found.id;
      } catch (lookup) {
        // Still unknown: the debit STANDS. Cash and credits can never both be
        // out; the next `charge.refunded` or a person settles it.
        const message = `${failure}; and the look-up failed: ${lookup instanceof Error ? lookup.message : String(lookup)}`;
        alarm(
          `refund leg ${leg.key} (${summary.grossCents} minor units ${summary.currency}) is in an UNKNOWN state — ${message}. ` +
            `The credits stay debited; find the refund in Stripe by its key and confirm or reverse the leg`
        );
        failed.push({ ...summary, state: 'unknown', message });
        continue;
      }
    }

    if (refundId !== undefined) {
      await deps.ledger.confirmCashoutLeg(leg.key, refundId);
      refunds.push({ ...summary, refundId });
    } else {
      // With the refund's id when one was made and failed: the ledger's copy of
      // Stripe's cumulative figure must fall with it (see THE REFUND INVARIANT).
      await deps.ledger.reverseCashoutLeg(
        leg.key,
        failedRefundId ? 'refund_failed' : ambiguous ? 'not_found_after_error' : 'stripe_refused',
        failedRefundId ? { refundId: failedRefundId } : undefined
      );
      if (failedRefundId) await resyncAfterFailure(deps.ledger, deps.stripe, leg.paymentIntentId, leg.key, alarm);
      failed.push({ ...summary, state: 'reversed', message: failure ?? 'refund failed' });
    }
  }

  // A1: a 0p leg makes no Stripe refund. It stands with the request (confirmed)
  // when some money went back, and is reversed with it when none did, so a
  // request that failed at Stripe never keeps credits for nothing.
  const anyRefunded = refunds.length > 0;
  for (const leg of plan.legs) {
    if (leg.grossCents !== 0n) continue;
    const summary = { paymentIntentId: leg.paymentIntentId, amountMicro: leg.amountMicro, grossCents: 0, currency: leg.currency };
    if (anyRefunded) {
      await deps.ledger.confirmCashoutLeg(leg.key, ZERO_LEG_REFUND_ID);
      refunds.push({ ...summary, refundId: ZERO_LEG_REFUND_ID });
    } else {
      await deps.ledger.reverseCashoutLeg(leg.key, 'no_money_leg_refunded');
      failed.push({ ...summary, state: 'reversed', message: 'nothing else in the request was refunded' });
    }
  }

  if (!anyRefunded) {
    return { status: 'stripe_error', message: failed.map((f) => f.message).join('; '), failed };
  }
  return {
    status: 'ok',
    refunds,
    failed,
    grossCents: refunds.reduce((sum, r) => sum + r.grossCents, 0),
    amountMicro: refunds.reduce((sum, r) => sum + r.amountMicro, 0n),
    byCurrency: byCurrencyOf(refunds),
  };
}

// ---------------------------------------------------------------------------
// Recovery of refund legs left `pending`. A leg is debited (journaled) BEFORE
// Stripe is called, so a crash before or during the call, or a look-up that
// could not answer, leaves the credits debited with the refund's fate unknown.
// Nothing in the request path comes back to it, so this sweep does: at boot
// (every pending leg then belongs to a process that is gone) and in the daily
// reconciliation.
//
// For each pending leg older than the grace period:
//   1. look the refund up by the key it carries (`metadata[fc1Key]`): if it
//      exists, confirm the leg;
//   2. if not, and the leg is younger than Stripe's 24-hour idempotency
//      window, POST the refund again under the SAME Idempotency-Key: Stripe
//      answers with the original refund if the first call did land, or makes
//      it now, so it can never be made twice;
//   3. otherwise (no refund exists and the key can no longer protect a retry)
//      reverse the leg: the credits return, and the customer can ask again.
// A step that cannot be answered leaves the leg pending for the next sweep.
// ---------------------------------------------------------------------------

/** Stripe keeps an Idempotency-Key for 24 hours; stay well inside it. */
export const REPOST_WITHIN_MS = 23 * 60 * 60 * 1000;

export interface LegRecoveryDeps {
  ledger: CreditsLedger;
  stripe: StripeApi;
  onAlarm: (message: string) => void;
  now?: () => number;
  /** A leg younger than this may belong to a request still in flight. 0 at
   *  boot; 15 minutes in the reconciliation. */
  graceMs: number;
}

export interface LegRecoveryReport {
  confirmed: number;
  reposted: number;
  reversed: number;
  stillPending: number;
}

export async function recoverPendingCashoutLegs(d: LegRecoveryDeps): Promise<LegRecoveryReport> {
  const now = (d.now ?? Date.now)();
  const report: LegRecoveryReport = { confirmed: 0, reposted: 0, reversed: 0, stillPending: 0 };
  for (const leg of d.ledger.allCashoutLegs()) {
    if (leg.state !== 'pending') continue;
    const ageMs = now - leg.atMs;
    if (ageMs < d.graceMs) {
      report.stillPending += 1;
      continue;
    }
    const what = `refund leg ${leg.key} (${leg.grossCents} minor units ${leg.currency}, user ${leg.userId})`;
    if (leg.grossCents === 0n) continue; // A1: settled below, with its request's money legs
    try {
      const found = await d.stripe.findRefund(leg.paymentIntentId, leg.key);
      if (found && found.status !== 'failed' && found.status !== 'canceled') {
        await d.ledger.confirmCashoutLeg(leg.key, found.id);
        report.confirmed += 1;
        d.onAlarm(`${what} was pending: Stripe has refund ${found.id} under its key — confirmed`);
        continue;
      }
      if (found) {
        // The refund was made and failed: nothing reached the card.
        await d.ledger.reverseCashoutLeg(leg.key, `recovered_${found.status}`, { refundId: found.id });
        await resyncAfterFailure(d.ledger, d.stripe, leg.paymentIntentId, leg.key, d.onAlarm);
        report.reversed += 1;
        d.onAlarm(`${what} was pending: Stripe's refund ${found.id} is ${found.status} — reversed, the credits are back`);
        continue;
      }
      if (ageMs < REPOST_WITHIN_MS) {
        try {
          const refund = await d.stripe.createRefund({
            paymentIntentId: leg.paymentIntentId,
            amountCents: Number(leg.grossCents),
            idempotencyKey: leg.key,
            metadata: { fc1Origin: 'cashout', fc1Key: leg.key, fc1UserId: leg.userId },
          });
          if (refund.status === 'failed' || refund.status === 'canceled') {
            await d.ledger.reverseCashoutLeg(leg.key, `recovered_${refund.status}`, { refundId: refund.id });
            await resyncAfterFailure(d.ledger, d.stripe, leg.paymentIntentId, leg.key, d.onAlarm);
            report.reversed += 1;
            d.onAlarm(`${what} was pending: the refund was made again and came back ${refund.status} — reversed`);
          } else {
            await d.ledger.confirmCashoutLeg(leg.key, refund.id);
            report.reposted += 1;
            d.onAlarm(`${what} was pending with no refund at Stripe: made under the same key (${refund.id}) — confirmed`);
          }
        } catch (e) {
          if (e instanceof StripeApiError && e.definitive) {
            await d.ledger.reverseCashoutLeg(leg.key, 'recovered_stripe_refused');
            report.reversed += 1;
            d.onAlarm(`${what} was pending: Stripe refused the refund (${e.message}) — reversed, the credits are back`);
          } else {
            throw e;
          }
        }
        continue;
      }
      await d.ledger.reverseCashoutLeg(leg.key, 'recovered_no_refund');
      report.reversed += 1;
      d.onAlarm(`${what} was pending for over 23 hours with no refund at Stripe — reversed, the credits are back`);
    } catch (e) {
      report.stillPending += 1;
      d.onAlarm(
        `${what} is still pending: recovery could not finish (${e instanceof Error ? e.message : String(e)}); it is tried again at the next sweep`
      );
    }
  }
  // A1: a 0p leg has no Stripe refund. It stands when a money leg of the same
  // request was refunded, is reversed when every money leg was reversed, and
  // waits while any is still pending (as the live path decides).
  const requestOf = (key: string): string => key.slice(0, key.lastIndexOf(':'));
  const legs = d.ledger.allCashoutLegs();
  for (const leg of legs) {
    if (leg.state !== 'pending' || leg.grossCents !== 0n) continue;
    if (now - leg.atMs < d.graceMs) continue; // counted as stillPending above
    const money = legs.filter((l) => l.grossCents !== 0n && requestOf(l.key) === requestOf(leg.key));
    if (money.some((l) => l.state === 'confirmed')) {
      await d.ledger.confirmCashoutLeg(leg.key, ZERO_LEG_REFUND_ID);
      report.confirmed += 1;
    } else if (money.every((l) => l.state === 'reversed')) {
      await d.ledger.reverseCashoutLeg(leg.key, 'no_money_leg_refunded');
      report.reversed += 1;
    } else {
      report.stillPending += 1;
    }
  }
  return report;
}

/** The boot sweep (fiat-session-service `buildBackend`): never throws, never
 *  blocks the boot. */
export async function recoverPendingCashoutLegsAtBoot(ledger: CreditsLedger): Promise<void> {
  const onAlarm = (m: string) => console.error(`[fiat-cashout] ALARM: ${m}`);
  try {
    if (!ledger.allCashoutLegs().some((l) => l.state === 'pending')) return;
    const report = await recoverPendingCashoutLegs({ ledger, stripe: makeStripeApi(), onAlarm, graceMs: 0 });
    console.log(
      `[fiat-cashout] boot recovery of pending refund legs: confirmed ${report.confirmed}, made ${report.reposted}, reversed ${report.reversed}, still pending ${report.stillPending}`
    );
  } catch (e) {
    onAlarm(`boot recovery of pending refund legs did not run: ${e instanceof Error ? e.message : String(e)}`);
  }
}

export interface CashoutService {
  request(request: { credential: string; amountMicro: bigint }): Promise<CashoutOutcome>;
  preview(request: { credential: string; amountMicro: bigint }): Promise<CashoutPreview>;
}

let overrideService: CashoutService | undefined;
let builtService: CashoutService | undefined;

export function setCashoutServiceForTest(service: CashoutService | undefined): void {
  overrideService = service;
}

/** The refund window in days (FIAT_REFUND_WINDOW_DAYS, default 90). */
export function refundWindowDays(): number {
  const raw = process.env.FIAT_REFUND_WINDOW_DAYS;
  const days = raw === undefined || raw === '' ? 90 : Number(raw);
  if (!Number.isInteger(days) || days <= 0) {
    throw new Error(`FIAT_REFUND_WINDOW_DAYS must be a positive integer, got "${raw}"`);
  }
  return days;
}

export async function getCashoutService(): Promise<CashoutService> {
  if (overrideService) return overrideService;
  if (!builtService) {
    const { ledger, credentials } = await getFiatDeps();
    const deps: CashoutDeps = { ledger, credentials, stripe: makeStripeApi(), refundWindowDays: refundWindowDays() };
    builtService = {
      request: (request) => requestCashout(deps, request),
      preview: (request) => previewCashout(deps, request),
    };
  }
  return builtService;
}
