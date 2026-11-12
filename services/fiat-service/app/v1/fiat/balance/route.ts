// FC1 — GET /v1/fiat/balance: the fiat-credits balance, for the account page
// to display. Returns micro-USDC as strings — the UI formats them.
//
// Legal list 2.13: the read is CLOSED. It answers the holder of the account's
// spending credential (Authorization: Bearer …; the same credential that
// authorises a refund), for that account only:
//   { availableMicro, owedMicro, refundableMicro, frozen }
// A request with no credential is refused. FIAT_BALANCE_OPEN=on restores the
// old lookup by `?address=` ({ availableMicro } only) for a client that has
// not moved to the credential yet; leave it unset in production.
import { getFiatDeps } from '../../../../src/lib/fiat-session-service';
import { fiatUserId } from '../../../../src/lib/fiat-identity';
import { refundWindowDays } from '../../../../src/lib/fiat-cashout-service';

const DAY_MS = 86_400_000;

export async function GET(req: Request): Promise<Response> {
  const header = req.headers.get('authorization');
  const open = process.env.FIAT_BALANCE_OPEN === 'on';

  if (!header?.startsWith('Bearer ')) {
    if (!open) return Response.json({ error: 'credential_required' }, { status: 401 });
    const address = new URL(req.url).searchParams.get('address');
    let userId: string;
    try {
      userId = fiatUserId(address ?? '');
    } catch {
      return Response.json({ error: 'address query param must be a valid address' }, { status: 400 });
    }
    let ledger;
    try {
      ({ ledger } = await getFiatDeps());
    } catch (e) {
      // The caller is not known yet: say nothing of the cause, and log it.
      console.error(`[fiat/balance] backend unavailable: ${e instanceof Error ? e.message : String(e)}`);
      return Response.json({ error: 'service_unavailable' }, { status: 503 });
    }
    const balance = ledger.availableMicro(userId);
    return Response.json({ availableMicro: (balance > 0n ? balance : 0n).toString() });
  }

  let deps;
  try {
    deps = await getFiatDeps();
  } catch (e) {
    // The caller is not known yet: say nothing of the cause, and log it.
    console.error(`[fiat/balance] backend unavailable: ${e instanceof Error ? e.message : String(e)}`);
    return Response.json({ error: 'service_unavailable' }, { status: 503 });
  }
  const userId = deps.credentials.authenticate(header.slice('Bearer '.length));
  if (!userId) return Response.json({ error: 'unauthorised' }, { status: 401 });

  const balance = deps.ledger.availableMicro(userId);
  return Response.json({
    // Never negative on the wire: a balance below zero is 0 to spend, and the
    // debt is its own figure.
    availableMicro: (balance > 0n ? balance : 0n).toString(),
    owedMicro: (balance < 0n ? -balance : 0n).toString(),
    // What a refund could return today: unspent credits on card charges
    // inside the refund window (the page shows it beside the refund form).
    refundableMicro: deps.ledger.refundableMicro(userId, { windowMs: refundWindowDays() * DAY_MS }).toString(),
    frozen: deps.ledger.isFrozen(userId),
  });
}
