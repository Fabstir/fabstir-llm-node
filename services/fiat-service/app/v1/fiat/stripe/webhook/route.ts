// FC1.4 — the Stripe webhook: the ONLY place ledger credits are created
// (Decision 6). Signature verified over the RAW body with node:crypto;
// idempotent per event id; unusable events are 200-and-logged so Stripe never
// retries them forever.
//
// TX1.2: what happens to a verified event lives in src/lib/stripe-webhook.ts
// (VAT, the country gate, refunds made outside this service, chargebacks).
// The endpoint must be subscribed to: checkout.session.completed,
// checkout.session.async_payment_succeeded, charge.refunded,
// charge.refund.updated, refund.updated, refund.failed,
// charge.dispute.created, charge.dispute.closed,
// charge.dispute.funds_withdrawn, charge.dispute.funds_reinstated; and created
// with API version 2026-08-26.dahlia (STRIPE_API_VERSION): events are rendered
// in the ENDPOINT's version, and that is the one this service reads.
import { verifyStripeSignature } from '../../../../../src/lib/stripe';
import { getFiatDeps } from '../../../../../src/lib/fiat-session-service';
import { handleStripeEvent, productionWebhookDeps } from '../../../../../src/lib/stripe-webhook';

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

  const { ledger } = await getFiatDeps();
  const result = await handleStripeEvent(productionWebhookDeps(ledger), event);
  if (typeof result.body.ignored === 'string') {
    console.log(`[fiat-stripe] ignored webhook: ${result.body.ignored}`);
  }
  return Response.json(result.body, { status: result.status });
}
