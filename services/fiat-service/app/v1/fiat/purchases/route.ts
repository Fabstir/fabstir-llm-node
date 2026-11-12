// GET /v1/fiat/purchases: the account page's "Your purchases" list (Terms
// section 2). Closed exactly like the balance (legal list 2.13): it answers
// the holder of the account's spending credential (Authorization: Bearer …),
// for that account only, and takes no address. A read of the ledger; no
// Stripe call.
import { getFiatDeps } from '../../../../src/lib/fiat-session-service';
import { purchaseHistory } from '../../../../src/lib/purchase-history';

export const dynamic = 'force-dynamic';

const NO_STORE = { 'cache-control': 'no-store' };

export async function GET(req: Request): Promise<Response> {
  const header = req.headers.get('authorization');
  if (!header?.startsWith('Bearer ')) {
    return Response.json({ error: 'credential_required' }, { status: 401, headers: NO_STORE });
  }
  let deps;
  try {
    deps = await getFiatDeps();
  } catch (e) {
    console.error(`[fiat/purchases] backend unavailable: ${e instanceof Error ? e.message : String(e)}`);
    return Response.json({ error: 'service_unavailable' }, { status: 503, headers: NO_STORE });
  }
  const userId = deps.credentials.authenticate(header.slice('Bearer '.length));
  if (!userId) return Response.json({ error: 'unauthorised' }, { status: 401, headers: NO_STORE });
  return Response.json(purchaseHistory(deps.ledger, userId), { headers: NO_STORE });
}
