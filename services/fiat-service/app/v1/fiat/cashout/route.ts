// FC1.4 — POST /v1/fiat/cashout: Stripe refund against the remaining ledger
// balance, never USDC (Decision 6). Same bearer credential as session-open.
import { getCashoutService } from '../../../../src/lib/fiat-cashout-service';

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
  const { amountMicro } = body ?? {};
  if (typeof amountMicro !== 'string' || !DIGITS_RE.test(amountMicro)) {
    return Response.json(
      { error: 'amountMicro must be a decimal string of USDC micro-units' },
      { status: 400 }
    );
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

  const outcome = await service.request({ credential, amountMicro: BigInt(amountMicro) });
  switch (outcome.status) {
    case 'ok':
      return Response.json({ refundId: outcome.refundId, amountMicro: outcome.amountMicro.toString() });
    case 'unauthorised':
      return Response.json({ error: 'unauthorised' }, { status: 401 });
    case 'refused':
      return Response.json({ error: 'refused', reason: outcome.reason }, { status: 403 });
    case 'stripe_error':
      return Response.json({ error: 'stripe_error', message: outcome.message }, { status: 502 });
  }
}
