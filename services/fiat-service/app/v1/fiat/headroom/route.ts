// D20 item 5 — GET /v1/fiat/headroom: what the credential's user can still
// open, so the helper can refuse a card batch it cannot pay for BEFORE any
// escrow (INSUFFICIENT_HEADROOM), and gate each open (INSUFFICIENT_BALANCE).
// `payerId` is the stable opaque payer the helper records per batch.
// `?kind=training` reports the training budget; `?kind=passes` (D21) the
// passes per-session cap with the STANDARD daily window it shares; absent =
// standard.
import { getFiatDeps } from '../../../../src/lib/fiat-session-service';
import { fiatPayerId } from '../../../../src/lib/fiat-identity';
import { capsFor, type SessionKind } from '../../../../src/lib/gatekeeper';

export async function GET(req: Request): Promise<Response> {
  const header = req.headers.get('authorization');
  if (!header?.startsWith('Bearer ')) {
    return Response.json({ error: 'missing bearer credential' }, { status: 401 });
  }
  const kindParam = new URL(req.url).searchParams.get('kind');
  if (kindParam !== null && kindParam !== 'training' && kindParam !== 'standard' && kindParam !== 'passes') {
    return Response.json({ error: "kind must be 'standard', 'training' or 'passes' when present" }, { status: 400 });
  }
  const kind: SessionKind = kindParam === 'training' || kindParam === 'passes' ? kindParam : 'standard';
  let deps;
  try {
    deps = await getFiatDeps();
  } catch (e) {
    return Response.json({ error: e instanceof Error ? e.message : 'fiat backend unavailable' }, { status: 503 });
  }
  const userId = deps.credentials.authenticate(header.slice('Bearer '.length));
  if (!userId) return Response.json({ error: 'unauthorised' }, { status: 401 });
  if (!deps.config) return Response.json({ error: 'fiat backend has no gatekeeper config' }, { status: 503 });

  const view = deps.ledger.headroomView(userId, kind);
  const caps = capsFor(deps.config, kind);
  return Response.json({
    payerId: fiatPayerId(userId),
    kind,
    availableMicro: view.availableMicro.toString(),
    spentInWindowMicro: view.spentInWindowMicro.toString(),
    dailyCapMicro: caps.maxDailySpendMicro.toString(),
    sessionCapMicro: caps.maxDepositPerSessionMicro.toString(),
    opensPerMinute: deps.config.maxOpensPerMinute,
  });
}
