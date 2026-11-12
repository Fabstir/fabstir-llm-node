// TX1.2 — what the webhook does with a verified Stripe event. The route is a
// thin shell (signature, JSON); everything that touches money is here, over
// injected deps, so it is tested without a network.
//
//   checkout.session.completed / async_payment_succeeded
//       tax status -> country gate -> location evidence -> credit the amount
//       BEFORE tax -> confirmation email. A refused purchase credits nothing
//       and is refunded in full under Idempotency-Key `gate:<session id>`.
//   charge.refunded            a refund made outside this service is debited
//                              as its net share (cumulative, never twice)
//   charge.refund.updated      status `failed`: what it debited comes back
//   charge.dispute.created     debit the disputed share, freeze the user
//   charge.dispute.closed      won restores exactly; lost keeps the debit
//   charge.dispute.funds_*     journaled for reconciliation
//
// The Stripe account is shared with Fabstir v2, so events for charges this
// ledger does not know arrive here too: they are ignored, quietly unless the
// charge carries OUR tag.
//
// Answers: 200 for anything handled or ignored (Stripe must not retry it);
// 5xx only when a retry can help: a gate refund that could not be made, or an
// event about a charge of OURS whose purchase this ledger has not journaled
// yet (its checkout.session.completed is late): Stripe redelivers, and the
// refund or dispute is applied once the purchase is known instead of being
// lost while the purchase later credits in full.
import type { CreditsLedger } from './ledger';
import {
  extractCheckoutFacts,
  extractPurchase,
  closedDisputeKeepsDebit,
  isAlreadyRefunded,
  lazyStripeApi,
  StripeApiError,
  type StripeApi,
  type Uncreditable,
} from './stripe';
import { allowedBillingCountries } from './sales-countries';
import {
  emailSenderFromEnv,
  sendPurchaseConfirmation,
  sendRefusalNotice,
  termsSourceFromEnv,
  type EmailSender,
  type TermsSource,
} from './purchase-confirmation';

const MICRO_PER_CENT = 10_000n;
const APP_TAG = 'platformless-ai';
/** The card-country read inside the webhook: Stripe expects an answer within
 *  seconds, and the evidence can be read again later, so it is cut short. */
export const CARD_COUNTRY_TIMEOUT_MS = 5_000;
/** How long an event about an unknown charge of ours is answered 503 (so
 *  Stripe redelivers it) before it is alarmed and let go. */
export const UNKNOWN_CHARGE_RETRY_MS = 6 * 60 * 60 * 1000;

export interface StripeWebhookDeps {
  ledger: CreditsLedger;
  stripe: StripeApi;
  onAlarm: (message: string) => void;
  /** Billing countries a purchase is credited for (TX1 §3). */
  allowedCountries: Set<string>;
  /** The confirmation email seam; absent = no provider configured. */
  email?: { sender?: EmailSender; terms?: TermsSource };
  log?: (message: string) => void;
  now?: () => number;
}

export interface WebhookResult {
  status: number;
  body: Record<string, unknown>;
}

interface StripeEvent {
  id: string;
  type: string;
  object: Record<string, unknown>;
  /** The event's own creation time, unix seconds. */
  createdS?: number;
}

function parseEvent(event: unknown): StripeEvent | undefined {
  const e = event as { id?: unknown; type?: unknown; data?: { object?: unknown } };
  if (typeof e?.id !== 'string' || typeof e.type !== 'string') return undefined;
  const object = e.data?.object;
  if (!object || typeof object !== 'object') return undefined;
  const created = (event as { created?: unknown }).created;
  return {
    id: e.id,
    type: e.type,
    object: object as Record<string, unknown>,
    ...(typeof created === 'number' ? { createdS: created } : {}),
  };
}

const ok = (body: Record<string, unknown>): WebhookResult => ({ status: 200, body: { received: true, ...body } });
const ignored = (reason: string): WebhookResult => ok({ ignored: reason });

function credits(micro: bigint): string {
  const sign = micro < 0n ? '-' : '';
  const abs = micro < 0n ? -micro : micro;
  const whole = abs / MICRO_PER_CENT;
  const frac = (abs % MICRO_PER_CENT).toString().padStart(4, '0').replace(/0+$/, '');
  return `${sign}${whole}${frac ? `.${frac}` : ''} credits`;
}

/** Whether a Stripe object's metadata says the payment is this product's. */
function isOurs(metadata: unknown): boolean {
  const m = (metadata ?? {}) as Record<string, unknown>;
  return m.app === APP_TAG || typeof m.fc1UserId === 'string';
}

/** Tagged as ANOTHER product's (the account is shared with Fabstir v2). */
function isAnotherProducts(metadata: unknown): boolean {
  const app = ((metadata ?? {}) as Record<string, unknown>).app;
  return typeof app === 'string' && app !== APP_TAG;
}

const safeCents = (v: unknown): number => (typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 ? v : 0);

/**
 * An event about a charge this ledger has no purchase for. Another product's
 * charge is ignored. A charge of OURS is one whose purchase event has not
 * arrived yet: answered 503 so Stripe redelivers the event after the purchase
 * is journaled. After six hours it is alarmed for a person and let go.
 * `hint` is what the event itself says about the charge; without one the
 * PaymentIntent is read from Stripe (a dispute or a refund carries no tags).
 */
async function unknownCharge(
  deps: StripeWebhookDeps,
  event: StripeEvent,
  pi: string,
  what: string,
  hint?: { metadata?: unknown; createdS?: unknown }
): Promise<WebhookResult> {
  const notOurs = ignored(`${what}: charge ${pi} is not in this ledger`);
  if (deps.ledger.purchaseKnown({ paymentIntentId: pi })) {
    // Known, but as a purchase that was REFUSED and refunded: there is no
    // charge to debit and nothing a redelivery would change.
    deps.onAlarm(`${what}: ${pi} is a purchase this service refused and refunded — look at it in the Stripe dashboard`);
    return notOurs;
  }
  if (hint && isAnotherProducts(hint.metadata)) return notOurs;
  let ours = hint ? isOurs(hint.metadata) : false;
  let createdS = hint && typeof hint.createdS === 'number' ? hint.createdS : undefined;
  if (!ours) {
    try {
      const state = await deps.stripe.paymentState(pi);
      ours = state.app === APP_TAG || state.userId !== undefined;
      createdS = state.createdS ?? createdS;
    } catch (e) {
      // Stripe says there is no such PaymentIntent for this key: not ours.
      if (e instanceof StripeApiError && e.httpStatus === 404) return notOurs;
      // Whose charge it is could not be established. Let Stripe ask again, but
      // not for ever: on a shared account these may be another product's
      // events, and an endpoint that keeps answering 503 is disabled by Stripe.
      // Bounded by the EVENT's age, which every event carries.
      const eventAgeMs = event.createdS === undefined ? Infinity : (deps.now ?? Date.now)() - event.createdS * 1000;
      const why = e instanceof Error ? e.message : String(e);
      if (eventAgeMs < UNKNOWN_CHARGE_RETRY_MS) {
        (deps.log ?? console.log)(`[fiat-stripe] ${what}: could not read ${pi} from Stripe (${why}); asking for redelivery`);
        return { status: 503, body: { error: 'payment_not_readable', retry: true } };
      }
      deps.onAlarm(
        `${what} (event ${event.id}): ${pi} could not be read from Stripe for over six hours (${why}); the event is let go — ` +
          `if the payment is ours, apply it by hand`
      );
      return notOurs;
    }
  }
  if (!ours) return notOurs;
  const madeS = createdS ?? event.createdS;
  const ageMs = madeS === undefined ? Infinity : (deps.now ?? Date.now)() - madeS * 1000;
  if (ageMs < UNKNOWN_CHARGE_RETRY_MS) {
    (deps.log ?? console.log)(
      `[fiat-stripe] ${what}: ${pi} is ours but its purchase is not journaled yet; asking Stripe to redeliver`
    );
    return { status: 503, body: { error: 'charge_not_known_yet', retry: true } };
  }
  deps.onAlarm(
    `${what}: ${pi} carries our tag but this ledger has had no purchase for it for over six hours — reconcile by hand`
  );
  return notOurs;
}

export async function handleStripeEvent(deps: StripeWebhookDeps, rawEvent: unknown): Promise<WebhookResult> {
  const event = parseEvent(rawEvent);
  if (!event) return ignored('event has no id, type or object');
  switch (event.type) {
    case 'checkout.session.completed':
    case 'checkout.session.async_payment_succeeded':
      return handleCheckout(deps, rawEvent, event);
    case 'charge.refunded':
      return handleChargeRefunded(deps, event);
    case 'charge.refund.updated':
    case 'refund.updated':
    case 'refund.failed':
      return handleRefundUpdated(deps, event);
    case 'charge.dispute.created':
      return handleDisputeCreated(deps, event);
    case 'charge.dispute.closed':
      return handleDisputeClosed(deps, event);
    case 'charge.dispute.funds_withdrawn':
    case 'charge.dispute.funds_reinstated':
      return handleDisputeFunds(deps, event);
    default:
      return ignored(`unhandled event type ${event.type}`);
  }
}

// ---- purchases -------------------------------------------------------------

async function handleCheckout(deps: StripeWebhookDeps, rawEvent: unknown, event: StripeEvent): Promise<WebhookResult> {
  const purchase = extractPurchase(rawEvent);
  if (purchase.kind === 'ignored') {
    // A PAID session of ours that cannot be credited (a currency other than US
    // dollars, an unusable tax figure, no user id) is money taken for nothing,
    // and Stripe will not send the event again: refund it in full, like the
    // country gate, and alarm. Anything else is another product's session.
    const metadata = (event.object.metadata ?? {}) as Record<string, unknown>;
    const ours = isOurs(metadata) || typeof metadata.fc1Credits === 'string';
    if (!purchase.uncreditable || !ours) return ignored(purchase.reason);
    if (deps.ledger.hasStripeEvent(event.id)) return ok({ applied: false });
    const lenient = extractCheckoutFacts(rawEvent);
    const pi = typeof event.object.payment_intent === 'string' ? event.object.payment_intent : undefined;
    if (deps.ledger.purchaseKnown({ sessionId: lenient.sessionId, paymentIntentId: pi })) return ok({ applied: false });
    deps.onAlarm(`paid checkout cannot be credited (${purchase.reason}) — refunding it in full`);
    return refusePurchase(
      deps,
      {
        eventId: event.id,
        userId: typeof metadata.fc1UserId === 'string' ? metadata.fc1UserId : '',
        ...(pi ? { paymentIntentId: pi } : {}),
        currency: String(event.object.currency ?? 'usd'),
      },
      { ...lenient, grossCents: safeCents(lenient.grossCents), taxCents: safeCents(lenient.taxCents) },
      purchase.uncreditable
    );
  }
  // A replay (Stripe redelivers on any slow answer): nothing is fetched,
  // refunded or journaled a second time.
  if (deps.ledger.hasStripeEvent(purchase.eventId)) return ok({ applied: false });

  const facts = extractCheckoutFacts(rawEvent);
  // The same occurrence under a second event id (Stripe can emit two event
  // objects for one), or `completed` then `async_payment_succeeded`: one
  // Checkout Session, and one PaymentIntent, is one purchase.
  const known = deps.ledger.purchaseKnown({ sessionId: facts.sessionId, paymentIntentId: purchase.paymentIntentId });
  if (known) return ok({ applied: false, duplicate: known });
  const where = `session ${facts.sessionId ?? '(no id)'} (${purchase.paymentIntentId ?? 'no payment intent'})`;

  // Tax status: Stripe Tax must have calculated. Otherwise the amount charged
  // carries the wrong VAT (usually none), so nothing is credited.
  if (facts.taxStatus !== 'complete') {
    deps.onAlarm(
      `${where}: automatic_tax.status is ${String(facts.taxStatus ?? 'absent')}, not complete — purchase refused and refunded; ` +
        `check that Stripe Tax is on and the registration is active`
    );
    return refusePurchase(deps, { ...purchase, currency: String(event.object.currency ?? 'usd') }, facts, 'tax_status');
  }
  // The country gate. Checkout cannot restrict billing countries, so this is
  // the enforcement; the account page's selector is presentation only.
  if (!facts.billingCountry || !deps.allowedCountries.has(facts.billingCountry)) {
    (deps.log ?? console.log)(
      `[fiat-stripe] ${where}: billing country ${facts.billingCountry ?? '(none)'} is outside the area we sell to — refused and refunded`
    );
    return refusePurchase(deps, { ...purchase, currency: String(event.object.currency ?? 'usd') }, facts, 'country');
  }

  // Location evidence: two pieces for a digital service's place of supply.
  // The card country comes from the charge; a failed read must not hold up a
  // paid customer's credits (Stripe keeps the evidence), so it only alarms.
  let cardCountry: string | undefined;
  if (purchase.paymentIntentId) {
    try {
      cardCountry = (
        await deps.stripe.cardCountry(purchase.paymentIntentId, { timeoutMs: CARD_COUNTRY_TIMEOUT_MS })
      )?.toUpperCase();
      if (!cardCountry) deps.onAlarm(`${where}: Stripe reported no card country — journaled with the billing country alone`);
    } catch (e) {
      deps.onAlarm(
        `${where}: card country evidence could not be read (${e instanceof Error ? e.message : String(e)}) — credited by the billing address; read it from Stripe by hand`
      );
    }
  }
  if (cardCountry && cardCountry !== facts.billingCountry) {
    deps.onAlarm(
      `${where}: location evidence disagrees — card issued in ${cardCountry}, billing address in ${facts.billingCountry}; credited by the billing address, review the place of supply`
    );
  }
  if (facts.creditsBought !== undefined && facts.subtotalCents !== undefined && facts.subtotalCents !== facts.creditsBought) {
    deps.onAlarm(
      `${where}: amount_subtotal ${facts.subtotalCents} is not the ${facts.creditsBought} credits bought — credited what was paid before tax`
    );
  }
  if (facts.billingCountry === 'GB' && facts.taxCents === 0) {
    deps.onAlarm(
      `${where}: a GB purchase carries zero VAT${facts.taxIdPresent ? ' (a tax id was given, but a UK business is still charged UK VAT)' : ''} — ` +
        `the GB registration or the tax code is missing in Stripe Tax; VAT is going uncollected`
    );
  }

  const { applied, duplicate } = await deps.ledger.purchase(purchase.userId, purchase.amountMicro, purchase.eventId, {
    ...(purchase.paymentIntentId ? { paymentIntentId: purchase.paymentIntentId } : {}),
    grossCents: facts.grossCents,
    taxCents: facts.taxCents,
    ...(facts.sessionId ? { sessionId: facts.sessionId } : {}),
    billingCountry: facts.billingCountry,
    ...(cardCountry ? { cardCountry } : {}),
    taxIdPresent: facts.taxIdPresent,
    ...(facts.consent ? { consent: facts.consent } : {}),
  });

  if (applied) {
    const outcome = await sendPurchaseConfirmation(
      { ...(deps.email ?? {}), onAlarm: deps.onAlarm },
      {
        ...(facts.email ? { to: facts.email } : {}),
        accountId: purchase.userId,
        netCents: facts.grossCents - facts.taxCents,
        taxCents: facts.taxCents,
        grossCents: facts.grossCents,
        currency: String(event.object.currency ?? 'usd'),
        purchasedAtMs: Date.now(),
        ...(purchase.paymentIntentId ? { paymentIntentId: purchase.paymentIntentId } : {}),
        ...(facts.consent ? { termsVersion: facts.consent.termsVersion, useNow: facts.consent.useNow } : {}),
      }
    );
    if (outcome === 'no_sender') {
      (deps.log ?? console.log)(`[fiat-stripe] ${where}: confirmation email not sent (no email provider is configured)`);
    }
  }
  return ok({ applied, ...(duplicate ? { duplicate } : {}) });
}

/** Credit nothing; refund the whole charge; journal the refusal. The refund
 *  comes FIRST: if it cannot be made the event is answered 5xx and nothing is
 *  journaled, so Stripe's retry runs this again. The Idempotency-Key makes a
 *  retry inside 24 h return the same refund; after that, Stripe answers
 *  `charge_already_refunded`, which is success here. */
async function refusePurchase(
  deps: StripeWebhookDeps,
  purchase: { eventId: string; userId: string; paymentIntentId?: string; currency?: string },
  facts: ReturnType<typeof extractCheckoutFacts>,
  reason: 'country' | 'tax_status' | Uncreditable
): Promise<WebhookResult> {
  let refundId: string | undefined;
  let refunded = false;
  if (!purchase.paymentIntentId) {
    deps.onAlarm(
      `refused purchase (event ${purchase.eventId}, ${reason}) has no payment intent — it cannot be refunded automatically; refund it in the Stripe dashboard`
    );
  } else {
    try {
      const refund = await deps.stripe.createRefund({
        paymentIntentId: purchase.paymentIntentId,
        idempotencyKey: `gate:${facts.sessionId ?? purchase.eventId}`,
        metadata: {
          fc1Origin: 'gate',
          fc1Reason: reason,
          ...(purchase.userId ? { fc1UserId: purchase.userId } : {}),
        },
      });
      refundId = refund.id;
      refunded = true;
    } catch (e) {
      if (isAlreadyRefunded(e)) {
        refunded = true;
      } else {
        deps.onAlarm(
          `gate refund for ${purchase.paymentIntentId} (${reason}) failed: ${e instanceof Error ? e.message : String(e)} — nothing credited; Stripe will redeliver the event`
        );
        return { status: 503, body: { error: 'refund_failed', retry: true } };
      }
    }
  }
  const { applied } = await deps.ledger.recordRefusedPurchase({
    userId: purchase.userId,
    eventId: purchase.eventId,
    ...(purchase.paymentIntentId ? { paymentIntentId: purchase.paymentIntentId } : {}),
    ...(facts.sessionId ? { sessionId: facts.sessionId } : {}),
    grossCents: facts.grossCents,
    taxCents: facts.taxCents,
    ...(facts.billingCountry ? { billingCountry: facts.billingCountry } : {}),
    reason,
    ...(refundId ? { refundId } : {}),
    refunded,
  });
  if (applied && refunded) {
    // Tell the buyer (the page they return to cannot know): by the same send
    // rules as the confirmation, so nothing goes until a provider exists.
    await sendRefusalNotice(
      { ...(deps.email?.sender ? { sender: deps.email.sender } : {}), onAlarm: deps.onAlarm },
      {
        ...(facts.email ? { to: facts.email } : {}),
        grossCents: facts.grossCents,
        currency: purchase.currency ?? 'usd',
        reason,
        salesArea: deps.allowedCountries.size > 1 ? 'the United Kingdom and the European Union' : 'the United Kingdom',
        ...(purchase.paymentIntentId ? { paymentIntentId: purchase.paymentIntentId } : {}),
      }
    );
  }
  return ok({ applied, refused: reason, refunded });
}

// ---- refunds made outside this service --------------------------------------

async function handleChargeRefunded(deps: StripeWebhookDeps, event: StripeEvent): Promise<WebhookResult> {
  const pi = event.object.payment_intent;
  const cumulative = event.object.amount_refunded;
  if (typeof pi !== 'string' || !pi) return ignored('charge has no payment intent');
  if (typeof cumulative !== 'number') return ignored('charge has no amount_refunded');
  const known = deps.ledger.charge(pi);
  if (!known && !deps.ledger.purchaseKnown({ paymentIntentId: pi })) {
    return unknownCharge(deps, event, pi, `charge.refunded (${cumulative} cents refunded)`, {
      metadata: event.object.metadata,
      createdS: event.object.created,
    });
  }
  let cumulativeNow = cumulative;
  if (known && known.refundFailures > 0) {
    // `amount_refunded` only ever goes DOWN when a refund fails. This charge
    // has had one fail, so this snapshot may predate the failure and overstate
    // what reached the card. EVERY snapshot for such a charge is replaced by
    // its state NOW: for the debit, and for the cumulative figure the ledger
    // keeps (an overstated one is debited later, when a pending leg is
    // reversed). Not only when it exceeds what the ledger has debited: a
    // snapshot equal to a NEW pending leg's amount looks explained and is not.
    try {
      const state = await deps.stripe.paymentState(pi);
      if (state.amountRefundedCents === undefined) throw new Error('Stripe gave no amount_refunded');
      cumulativeNow = state.amountRefundedCents;
    } catch (e) {
      (deps.log ?? console.log)(
        `[fiat-stripe] charge.refunded for ${pi}: the charge's current state could not be read (${e instanceof Error ? e.message : String(e)}); asking for redelivery`
      );
      return { status: 503, body: { error: 'payment_not_readable', retry: true } };
    }
  }
  const r = await deps.ledger.chargeRefunded(pi, cumulativeNow, event.id);
  if (!r.known) return ignored(`charge ${pi} is not in this ledger`);
  if (r.refusedPurchase) return ok({ applied: false, refusedPurchase: true });
  if (r.applied) {
    deps.onAlarm(
      `a refund of ${r.unexplainedCents} cents on ${pi} was made outside this service (the Stripe dashboard): ` +
        `${credits(r.debitedMicro)} debited from ${r.userId}`
    );
    if (r.balanceMicro < 0n) {
      deps.onAlarm(`${r.userId} is below zero (${credits(r.balanceMicro)}) after the refund on ${pi}: the credits were already spent`);
    }
  }
  return ok({ applied: r.applied });
}

async function handleRefundUpdated(deps: StripeWebhookDeps, event: StripeEvent): Promise<WebhookResult> {
  // `canceled` returns nothing to the card either (Stripe lowers the charge's
  // amount_refunded for both).
  if (event.object.status !== 'failed' && event.object.status !== 'canceled') {
    return ignored(`refund status ${String(event.object.status)} needs no action`);
  }
  const refundId = event.object.id;
  const pi = event.object.payment_intent;
  const amount = event.object.amount;
  if (typeof refundId !== 'string' || typeof pi !== 'string' || typeof amount !== 'number') {
    return ignored('failed refund lacks an id, payment intent or amount');
  }
  const metadata = (event.object.metadata ?? {}) as Record<string, unknown>;
  if (!Number.isSafeInteger(amount) || amount < 0) return ignored(`failed refund ${refundId} carries amount ${String(amount)}`);
  const r = await deps.ledger.refundFailed({
    refundId,
    paymentIntentId: pi,
    amountCents: amount,
    ...(typeof metadata.fc1Key === 'string' ? { key: metadata.fc1Key } : {}),
    eventId: event.id,
  });
  if (r.kind === 'unknown') {
    return unknownCharge(deps, event, pi, `refund ${refundId} failed`, isOurs(metadata) ? { metadata } : undefined);
  }
  if (r.kind !== 'refused-purchase') {
    // A refund failure is the one moment the ledger's picture of the charge can
    // be wrong in the dangerous direction: a refund made OUTSIDE this service
    // may have been hidden behind the leg that has just been reversed (its
    // `charge.refunded` looked explained by it), and no further event will come
    // to correct that. So read the charge as it is now and debit whatever the
    // ledger has not. Run on a redelivery too (it is idempotent).
    const resync = await resyncCharge(deps, event, pi);
    if (resync) return resync;
  }
  if (r.kind === 'refused-purchase') {
    deps.onAlarm(
      `refund ${refundId} of the REFUSED purchase ${pi} failed to reach the card: ${r.userId} has neither credits nor their money — refund them another way`
    );
    return ok({ applied: false });
  }
  if (r.applied) {
    deps.onAlarm(
      `refund ${refundId} (${amount} cents) on ${pi} never reached the card: ${credits(r.creditedMicro)} credited back to ${r.userId} — ` +
        `the customer is owed that refund another way if they still want it`
    );
  }
  return ok({ applied: r.applied });
}

/** After a refund failure: bring the ledger to the charge's CURRENT refunded
 *  figure. Answers a result only when the event must be redelivered (the read
 *  failed and the event is still young); otherwise undefined. */
async function resyncCharge(deps: StripeWebhookDeps, event: StripeEvent, pi: string): Promise<WebhookResult | undefined> {
  let refundedNow: number | undefined;
  try {
    refundedNow = (await deps.stripe.paymentState(pi)).amountRefundedCents;
  } catch (e) {
    const why = e instanceof Error ? e.message : String(e);
    const eventAgeMs = event.createdS === undefined ? Infinity : (deps.now ?? Date.now)() - event.createdS * 1000;
    if (eventAgeMs < UNKNOWN_CHARGE_RETRY_MS) {
      (deps.log ?? console.log)(`[fiat-stripe] ${pi}: could not be read after a refund failure (${why}); asking for redelivery`);
      return { status: 503, body: { error: 'payment_not_readable', retry: true } };
    }
    deps.onAlarm(
      `${pi} (event ${event.id}): the charge could not be read after a refund failure for over six hours (${why}) — ` +
        `compare its refunds in Stripe with the ledger by hand`
    );
    return undefined;
  }
  if (refundedNow === undefined) return undefined;
  const sync = await deps.ledger.chargeRefunded(pi, refundedNow, `resync:${event.id}`);
  if (sync.known && !sync.refusedPurchase && sync.applied) {
    deps.onAlarm(
      `after a refund failure on ${pi}, Stripe shows ${sync.unexplainedCents} cents refunded that the ledger had not debited ` +
        `(a refund made outside this service): ${credits(sync.debitedMicro)} debited from ${sync.userId}`
    );
  }
  return undefined;
}

// ---- disputes ---------------------------------------------------------------

function disputeFields(event: StripeEvent): { disputeId: string; pi: string; amount: number; status: string } | undefined {
  const { id, payment_intent: pi, amount, status } = event.object;
  if (typeof id !== 'string' || typeof pi !== 'string' || typeof amount !== 'number') return undefined;
  if (!Number.isSafeInteger(amount) || amount <= 0) return undefined;
  return { disputeId: id, pi, amount, status: typeof status === 'string' ? status : '' };
}

async function handleDisputeCreated(deps: StripeWebhookDeps, event: StripeEvent): Promise<WebhookResult> {
  const d = disputeFields(event);
  if (!d) return ignored('dispute lacks an id, payment intent or amount');
  const r = await deps.ledger.disputeOpened({
    disputeId: d.disputeId,
    paymentIntentId: d.pi,
    amountCents: d.amount,
    eventId: event.id,
  });
  if (!r.known) return unknownCharge(deps, event, d.pi, `dispute ${d.disputeId}`);
  if (r.applied) {
    deps.onAlarm(
      `dispute ${d.disputeId} opened on ${d.pi} for ${d.amount} cents: ${credits(r.debitedMicro)} debited from ${r.userId}, ` +
        `who is frozen until it closes (balance now ${credits(r.balanceMicro)}) — answer it in the Stripe dashboard before its deadline`
    );
  }
  return ok({ applied: r.applied });
}

async function handleDisputeClosed(deps: StripeWebhookDeps, event: StripeEvent): Promise<WebhookResult> {
  const d = disputeFields(event);
  if (!d) return ignored('dispute lacks an id, payment intent or amount');
  // The dispute IS closed (that is what the event says); the status says how.
  //   won, warning_closed (an inquiry that never became a chargeback): the
  //     funds were not taken, so the debit is restored;
  //   lost: the debit stands;
  //   prevented (resolved before a chargeback) and charge_refunded (an older
  //     API's name for an inquiry closed by refunding the charge): the
  //     cardholder has the money back, so the debit stands;
  //   anything else: closed with the debit standing, and an alarm. Never
  //     "left open": that would keep the user frozen for ever.
  const outcome: 'won' | 'lost' = closedDisputeKeepsDebit(d.status) ? 'lost' : 'won';
  const refundedToCardholder = d.status === 'prevented' || d.status === 'charge_refunded';
  // The closed event can overtake the created one: apply the opening first
  // (idempotent by dispute id), so the close always has something to close.
  const opened = await deps.ledger.disputeOpened({
    disputeId: d.disputeId,
    paymentIntentId: d.pi,
    amountCents: d.amount,
    eventId: `${event.id}:opened`,
  });
  if (!opened.known) return unknownCharge(deps, event, d.pi, `dispute ${d.disputeId} closed`);
  if (outcome === 'lost' && d.status !== 'lost' && !refundedToCardholder) {
    deps.onAlarm(
      `dispute ${d.disputeId} closed with status "${d.status}", which this service does not know: treated as closed with the debit standing — check it in the Stripe dashboard`
    );
  }
  const r = await deps.ledger.disputeClosed({ disputeId: d.disputeId, outcome, eventId: event.id });
  if (r.known && r.applied) {
    if (outcome === 'won') {
      (deps.log ?? console.log)(
        `[fiat-stripe] dispute ${d.disputeId} won: ${credits(r.restoredMicro)} restored to ${r.userId}, unfrozen` +
          (r.refundDebitedMicro > 0n
            ? `; ${credits(r.refundDebitedMicro)} debited for the refund made while it was open (balance ${credits(r.balanceMicro)})`
            : '')
      );
    } else if (refundedToCardholder) {
      (deps.log ?? console.log)(
        `[fiat-stripe] dispute ${d.disputeId} closed as ${d.status}: the cardholder was refunded, so the debit stands; ${r.userId} is unfrozen (balance ${credits(r.balanceMicro)})`
      );
    } else if (r.balanceMicro < 0n) {
      deps.onAlarm(
        `dispute ${d.disputeId} lost: ${r.userId} stays below zero (${credits(r.balanceMicro)}); new sessions and refunds are refused until it is made good`
      );
    } else {
      deps.onAlarm(`dispute ${d.disputeId} lost: the debit stands; ${r.userId} is unfrozen (balance ${credits(r.balanceMicro)})`);
    }
  }
  return ok({ applied: r.known && r.applied });
}

async function handleDisputeFunds(deps: StripeWebhookDeps, event: StripeEvent): Promise<WebhookResult> {
  const d = disputeFields(event);
  if (!d) return ignored('dispute lacks an id, payment intent or amount');
  const opened = await deps.ledger.disputeOpened({
    disputeId: d.disputeId,
    paymentIntentId: d.pi,
    amountCents: d.amount,
    eventId: `${event.id}:opened`,
  });
  if (!opened.known) return unknownCharge(deps, event, d.pi, `dispute ${d.disputeId} funds`);
  const r = await deps.ledger.disputeFunds({
    disputeId: d.disputeId,
    kind: event.type === 'charge.dispute.funds_withdrawn' ? 'withdrawn' : 'reinstated',
    cents: d.amount,
    eventId: event.id,
  });
  return ok({ applied: r.applied });
}

// ---- wiring -----------------------------------------------------------------

type WebhookOverride = Partial<Pick<StripeWebhookDeps, 'stripe' | 'onAlarm' | 'email'>>;
let override: WebhookOverride | undefined;

/** Tests inject the Stripe client and the alarm sink the route uses. */
export function setStripeWebhookForTest(o: WebhookOverride | undefined): void {
  override = o;
}

export function stripeAlarm(message: string): void {
  console.error(`[fiat-stripe] ALARM: ${message}`);
}

/** The route's deps: this ledger, the env-built Stripe client, the alarm sink
 *  the other checks use (the service journal), and the countries from
 *  SALES_EU, read per event. */
export function productionWebhookDeps(ledger: CreditsLedger): StripeWebhookDeps {
  const sender = emailSenderFromEnv();
  const terms = termsSourceFromEnv();
  return {
    ledger,
    stripe: override?.stripe ?? lazyStripeApi(),
    onAlarm: override?.onAlarm ?? stripeAlarm,
    allowedCountries: allowedBillingCountries(),
    email: override?.email ?? { ...(sender ? { sender } : {}), ...(terms ? { terms } : {}) },
  };
}
