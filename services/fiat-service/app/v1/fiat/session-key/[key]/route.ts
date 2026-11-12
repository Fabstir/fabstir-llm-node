// D20 item 5 — GET /v1/fiat/session-key/{key}: "what happened to this payment".
// Read-only and per user: the bearer credential's user is the only namespace
// searched, so one user can never learn anything about another's keys. The
// helper's card watch asks this after an open failed in an unknown state.
//   none            — no attempt with this key is on record (or a done one aged out)
//   pending         — an attempt may still escrow; the reconciler resolves it
//   done(jobId)     — the key is bound to that session
// plus the state of the hold opened under the key, when there is one.
import { getFiatDeps } from '../../../../../src/lib/fiat-session-service';

export async function GET(req: Request, ctx: { params: { key: string } }): Promise<Response> {
  const header = req.headers.get('authorization');
  if (!header?.startsWith('Bearer ')) {
    return Response.json({ error: 'missing bearer credential' }, { status: 401 });
  }
  // Next 14.2 has ALREADY percent-decoded the dynamic segment: decoding it
  // again would change a key that contains a literal '%' (or refuse it).
  const key = ctx.params.key ?? '';
  if (key.length === 0 || key.length > 200) {
    return Response.json({ error: 'key must be 1..200 characters' }, { status: 400 });
  }
  let deps;
  try {
    deps = await getFiatDeps();
  } catch (e) {
    return Response.json({ error: e instanceof Error ? e.message : 'fiat backend unavailable' }, { status: 503 });
  }
  const userId = deps.credentials.authenticate(header.slice('Bearer '.length));
  if (!userId) return Response.json({ error: 'unauthorised' }, { status: 401 });
  if (!deps.idempotency) return Response.json({ error: 'idempotency keys are not enabled' }, { status: 501 });

  const record = await deps.idempotency.lookup(userId, key);
  const hold = deps.ledger.holdForKey(userId, key);
  const holdState = hold ? { holdState: hold.state } : {};
  if (!record) return Response.json({ state: 'none', ...holdState });
  if (record.state === 'pending') return Response.json({ state: 'pending', ...holdState });
  return Response.json({ state: 'done', jobId: record.jobId.toString(), ...holdState });
}
