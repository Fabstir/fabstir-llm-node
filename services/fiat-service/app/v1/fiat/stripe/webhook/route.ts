// FC1.4 — the Stripe webhook: the ONLY place ledger credits are created
// (Decision 6). Signature verified over the RAW body with node:crypto;
// idempotent per event id; unusable events are 200-and-logged so Stripe never
// retries them forever.
import { extractPurchase, verifyStripeSignature } from '../../../../../src/lib/stripe';
import { getFiatDeps } from '../../../../../src/lib/fiat-session-service';

export async function POST(req: Request): Promise<Response> {
  const secret = process.env.FIAT_STRIPE_WEBHOOK_SECRET;
  if (!secret) {
    return Response.json(
      { error: 'FIAT_STRIPE_WEBHOOK_SECRET is not set — the fiat backend is not configured' },
      { status: 503 }
    );
  }

  const payload = await req.text();
  const signature = req.headers.get('stripe-signature') ?? '';
  if (!verifyStripeSignature(payload, signature, secret)) {
    return Response.json({ error: 'invalid stripe signature' }, { status: 400 });
  }

  let event: unknown;
  try {
    event = JSON.parse(payload);
  } catch {
    return Response.json({ error: 'body is not JSON' }, { status: 400 });
  }

  const extracted = extractPurchase(event);
  if (extracted.kind === 'ignored') {
    console.log(`[fiat-stripe] ignored webhook: ${extracted.reason}`);
    return Response.json({ received: true, ignored: extracted.reason });
  }

  const { ledger } = await getFiatDeps();
  const { applied } = await ledger.purchase(extracted.userId, extracted.amountMicro, extracted.eventId, {
    ...(extracted.paymentIntentId ? { paymentIntentId: extracted.paymentIntentId } : {}),
  });
  return Response.json({ received: true, applied });
}
