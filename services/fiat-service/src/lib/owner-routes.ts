// OP1.3a: the two relayer routes, POST /v1/fiat/owners/prepare and
// POST /v1/fiat/owners/submit (plan D14). Who may use them (R2):
//  - the relayer is configured (else 503 relayer_not_configured);
//  - Authorization: Bearer <the account's BROWSER card credential> (a helper
//    credential, or none, is 401);
//  - that account has at least one purchase still credited, i.e. not refunded
//    in full and not disputed (else 403
//    purchase_required). An invitation alone is not enough here: the gas
//    wallet pays for every change.
// The account is the credential's; nothing in the body can name another.
import { getFiatDeps } from './fiat-session-service';
import { getOwnerRelayer } from './owner-relayer';
import type { CreditsLedger } from './ledger';

/** A purchase that still stands: not refunded in full and with no open or lost
 *  dispute (the ledger's own `disputed`, the rule that blocks its refunds). A
 *  refunded or charged-back purchase does not unlock the relayer (review M5). */
export function hasCreditedPurchase(ledger: CreditsLedger, userId: string): boolean {
  return ledger.allCharges().some((c) => c.userId === userId && !c.disputed && c.refundedGrossCents < c.grossCents);
}

const NO_STORE = { 'cache-control': 'no-store' };
const json = (status: number, body: Record<string, unknown>) => Response.json(body, { status, headers: NO_STORE });

export async function ownerRoute(req: Request, step: 'prepare' | 'submit'): Promise<Response> {
  let relayer;
  try {
    relayer = await getOwnerRelayer();
  } catch (e) {
    console.error(`[fiat/owners] relayer unavailable: ${e instanceof Error ? e.message.split('\n')[0] : String(e)}`);
    return json(503, { error: 'service_unavailable' });
  }
  if (!relayer) return json(503, { error: 'relayer_not_configured' });

  const header = req.headers.get('authorization');
  if (!header?.startsWith('Bearer ')) return json(401, { error: 'credential_required' });
  let deps;
  try {
    deps = await getFiatDeps();
  } catch (e) {
    console.error(`[fiat/owners] backend unavailable: ${e instanceof Error ? e.message : String(e)}`);
    return json(503, { error: 'service_unavailable' });
  }
  // The browser's credential only: the helper's lives in local software and was
  // meant to expose a capped balance, nothing more (OP1.3a review L6).
  const userId = deps.credentials.authenticateFor(header.slice('Bearer '.length), 'browser');
  if (!userId) return json(401, { error: 'unauthorised' });
  if (!hasCreditedPurchase(deps.ledger, userId)) return json(403, { error: 'purchase_required' });

  let body: unknown;
  try {
    body = await req.json();
  } catch {
    return json(400, { error: 'bad_request' });
  }
  const out = step === 'prepare' ? await relayer.prepare(userId, body) : await relayer.submit(userId, body);
  return json(out.status, out.body);
}
