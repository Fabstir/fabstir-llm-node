// FC1 — POST /v1/fiat/purchase: start a card purchase of credits. Creates a
// Stripe Checkout Session server-side (secret key) tagged with the buyer's
// fc1UserId, and returns its URL for the browser to redirect to. The webhook
// (checkout.session.completed) is what actually credits the ledger, once the
// card is charged — this route only starts the flow.
//
// No auth: creating a Checkout Session charges nobody until the buyer completes
// it, and crediting a balance is harmless (the payer pays). SPENDING requires a
// credential, which is the security boundary. fc1UserId is the buyer's
// smart-account address (see fiat-identity.ts) so this tops up exactly the
// balance the helper later spends.
//
// TX1.1: Stripe Tax adds VAT on top of the credits' price; the buyer's consent
// (Terms version, 18 or over, "use my credits straight away": all three
// required) rides in the session metadata so the webhook journals it with the
// purchase; and checkout starts are rationed per account and per client address.
import { makeStripeCheckoutClient, type PurchaseConsent } from '../../../../src/lib/stripe';
import { fiatUserId } from '../../../../src/lib/fiat-identity';
import { clientIp, takeCheckoutStart } from '../../../../src/lib/purchase-rate-limit';

const MIN_CREDITS = 50; // Stripe's $0.50 minimum charge (1 credit = 1 cent)

function maxCredits(): number {
  const raw = process.env.FIAT_MAX_PURCHASE_CREDITS;
  const n = raw ? Number(raw) : 100_000; // default $1,000
  return Number.isInteger(n) && n > 0 ? n : 100_000;
}

const TERMS_VERSION_RE = /^[A-Za-z0-9._:-]{1,40}$/;
const EMAIL_RE = /^[^\s@]{1,64}@[^\s@]{1,255}$/;

/** Consent is required unless FIAT_CONSENT_REQUIRED=off (a transition switch
 *  for a client that does not send it yet; such purchases record no consent). */
function consentRequired(): boolean {
  return process.env.FIAT_CONSENT_REQUIRED !== 'off';
}

/** The consent block of the request: a valid one, `undefined` when absent, or
 *  'invalid' when present but not a recorded agreement. */
function parseConsent(raw: unknown): PurchaseConsent | undefined | 'invalid' {
  if (raw === undefined || raw === null) return undefined;
  if (typeof raw !== 'object') return 'invalid';
  const c = raw as { termsVersion?: unknown; over18?: unknown; useNow?: unknown };
  if (typeof c.termsVersion !== 'string' || !TERMS_VERSION_RE.test(c.termsVersion)) return 'invalid';
  if (c.over18 !== true) return 'invalid';
  // The "use my credits straight away" box must be ticked to buy, like the
  // other two statements (owner decision, 2026-11-12).
  if (c.useNow !== true) return 'invalid';
  return { termsVersion: c.termsVersion, over18: true, useNow: true };
}

export async function POST(req: Request): Promise<Response> {
  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return Response.json({ error: 'body must be JSON' }, { status: 400 });
  }

  const { clientAddress, credits, email } = body ?? {};
  let userId: string;
  try {
    userId = fiatUserId(clientAddress as string);
  } catch {
    return Response.json({ error: 'clientAddress must be a valid address' }, { status: 400 });
  }
  if (typeof credits !== 'number' || !Number.isInteger(credits)) {
    return Response.json({ error: 'credits must be an integer' }, { status: 400 });
  }
  if (credits < MIN_CREDITS || credits > maxCredits()) {
    return Response.json(
      { error: `credits must be between ${MIN_CREDITS} and ${maxCredits()}` },
      { status: 400 }
    );
  }

  const consent = parseConsent((body ?? {}).consent);
  if (consent === 'invalid' || (consent === undefined && consentRequired())) {
    return Response.json(
      {
        error: 'consent_required',
        message:
          'consent must carry termsVersion (the version of the Terms accepted), over18: true and useNow: true (the "use my credits straight away" statement); all three are required to buy',
      },
      { status: 400 }
    );
  }
  if (email !== undefined && (typeof email !== 'string' || !EMAIL_RE.test(email))) {
    return Response.json({ error: 'email must be an email address when present' }, { status: 400 });
  }

  const origin = new URL(req.url).origin;
  const successUrl = process.env.FIAT_PURCHASE_SUCCESS_URL ?? `${origin}/account?purchase=success`;
  const cancelUrl = process.env.FIAT_PURCHASE_CANCEL_URL ?? `${origin}/account?purchase=cancelled`;

  let checkout;
  try {
    checkout = makeStripeCheckoutClient();
  } catch (e) {
    return Response.json(
      { error: e instanceof Error ? e.message : 'fiat backend unavailable' },
      { status: 503 }
    );
  }

  // Counted last, so a request refused above never uses the budget.
  const slot = takeCheckoutStart(userId, clientIp(req));
  if (!slot.ok) {
    return Response.json(
      { error: 'rate_limited', message: 'too many purchases started in the last hour; try again later' },
      { status: 429, headers: { 'retry-after': String(slot.retryAfterS) } }
    );
  }

  try {
    // credits == cents before tax (1 credit = 1 US cent before VAT).
    const session = await checkout.createCheckoutSession({
      fc1UserId: userId,
      amountCents: credits,
      successUrl,
      cancelUrl,
      ...(consent ? { consent } : {}),
      ...(typeof email === 'string' ? { customerEmail: email } : {}),
    });
    return Response.json({ url: session.url });
  } catch (e) {
    // No session was created, so the start was not used: give it back, or five
    // Stripe errors would lock an honest buyer out for an hour.
    slot.release();
    return Response.json(
      { error: 'checkout_error', message: e instanceof Error ? e.message : String(e) },
      { status: 502 }
    );
  }
}
