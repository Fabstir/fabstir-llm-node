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
import { makeStripeCheckoutClient } from '../../../../src/lib/stripe';
import { fiatUserId } from '../../../../src/lib/fiat-identity';

const MIN_CREDITS = 50; // Stripe's $0.50 minimum charge (1 credit = 1 cent)

function maxCredits(): number {
  const raw = process.env.FIAT_MAX_PURCHASE_CREDITS;
  const n = raw ? Number(raw) : 100_000; // default $1,000
  return Number.isInteger(n) && n > 0 ? n : 100_000;
}

export async function POST(req: Request): Promise<Response> {
  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return Response.json({ error: 'body must be JSON' }, { status: 400 });
  }

  const { clientAddress, credits } = body ?? {};
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

  try {
    // credits == cents (1 credit = 1 cent = 1/100 USDC of spending power).
    const session = await checkout.createCheckoutSession({
      fc1UserId: userId,
      amountCents: credits,
      successUrl,
      cancelUrl,
    });
    return Response.json({ url: session.url });
  } catch (e) {
    return Response.json(
      { error: 'checkout_error', message: e instanceof Error ? e.message : String(e) },
      { status: 502 }
    );
  }
}
