// FC1.4 — POST /v1/fiat/cashout: Stripe refund against the remaining ledger
// balance, never USDC (Decision 6). Same bearer credential as session-open.
//
// TX1: the refund returns the VAT paid on the credits too and may span several
// card charges; the answer is { refunds[], grossCents, failed[] }. With
// `preview: true` in the body nothing moves: the answer is what WOULD return.
import { getCashoutService, type FailedLeg } from '../../../../src/lib/fiat-cashout-service';

const DIGITS_RE = /^\d+$/;

export async function POST(req: Request): Promise<Response> {
  const header = req.headers.get('authorization');
  if (!header?.startsWith('Bearer ')) {
    return Response.json({ error: 'missing bearer credential' }, { status: 401 });
  }
  const credential = header.slice('Bearer '.length);

  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return Response.json({ error: 'body must be JSON' }, { status: 400 });
  }
  const { amountMicro, preview } = body ?? {};
  if (typeof amountMicro !== 'string' || !DIGITS_RE.test(amountMicro)) {
    return Response.json(
      { error: 'amountMicro must be a decimal string of USDC micro-units' },
      { status: 400 }
    );
  }
  if (preview !== undefined && typeof preview !== 'boolean') {
    return Response.json({ error: 'preview must be a boolean when present' }, { status: 400 });
  }

  let service;
  try {
    service = await getCashoutService();
  } catch (e) {
    return Response.json(
      { error: e instanceof Error ? e.message : 'fiat backend unavailable' },
      { status: 503 }
    );
  }

  if (preview === true) {
    // TX1 §4: the gross that WOULD return, VAT included, before confirming.
    const quote = await service.preview({ credential, amountMicro: BigInt(amountMicro) });
    switch (quote.status) {
      case 'ok':
        return Response.json({
          preview: true,
          amountMicro: quote.amountMicro.toString(),
          grossCents: quote.grossCents,
          legs: quote.legs.map((l) => ({
            paymentIntentId: l.paymentIntentId,
            amountMicro: l.amountMicro.toString(),
            grossCents: l.grossCents,
          })),
        });
      case 'unauthorised':
        return Response.json({ error: 'unauthorised' }, { status: 401 });
      case 'refused':
        return Response.json({ error: 'refused', reason: quote.reason }, { status: 403 });
    }
  }

  const outcome = await service.request({ credential, amountMicro: BigInt(amountMicro) });
  const failedLegs = (failed: FailedLeg[]) =>
    failed.map((f) => ({
      paymentIntentId: f.paymentIntentId,
      amountMicro: f.amountMicro.toString(),
      grossCents: f.grossCents,
      state: f.state,
      message: f.message,
    }));
  switch (outcome.status) {
    case 'ok':
      return Response.json({
        // TX1 §5: one entry per card charge, the gross total, and any leg
        // that did not go through.
        refunds: outcome.refunds.map((r) => ({
          refundId: r.refundId,
          paymentIntentId: r.paymentIntentId,
          amountMicro: r.amountMicro.toString(),
          grossCents: r.grossCents,
        })),
        grossCents: outcome.grossCents,
        failed: failedLegs(outcome.failed),
        // Kept for clients written before TX1: the first refund and the total
        // credits refunded.
        refundId: outcome.refunds[0]!.refundId,
        amountMicro: outcome.amountMicro.toString(),
      });
    case 'unauthorised':
      return Response.json({ error: 'unauthorised' }, { status: 401 });
    case 'refused':
      return Response.json({ error: 'refused', reason: outcome.reason }, { status: 403 });
    case 'stripe_error':
      return Response.json(
        { error: 'stripe_error', message: outcome.message, failed: failedLegs(outcome.failed) },
        { status: 502 }
      );
  }
}
