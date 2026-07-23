// POST /v1/fiat/settlement/tick — run ONE settlement tick, now.
//
// The external heartbeat of the settlement listener. The in-process timer loop
// froze four times on 2026-07-23 (two builds, identical signature: startup tick
// completes, the next never begins), so the money path no longer depends on it:
// cron kicks this endpoint once a minute, each kick a fresh request context
// executing the SAME guarded tick (watchdog-capped, idempotent applies) against
// the SAME in-memory ledger. Server-to-server only: guarded by FIAT_ADMIN_TOKEN,
// deliberately NOT in the middleware's browser-CORS route set.
import { getProductionSettlementListener } from '../../../../../src/lib/settlement-listener';

export async function POST(req: Request): Promise<Response> {
  const token = process.env.FIAT_ADMIN_TOKEN;
  if (!token) {
    return Response.json({ error: 'FIAT_ADMIN_TOKEN is not set — tick endpoint disabled' }, { status: 503 });
  }
  const auth = req.headers.get('authorization') ?? '';
  if (auth !== `Bearer ${token}`) {
    return Response.json({ error: 'unauthorised' }, { status: 401 });
  }
  const listener = getProductionSettlementListener();
  if (!listener) {
    return Response.json(
      { error: 'settlement listener is not running (FIAT_SETTLEMENT_ENABLED unset?)' },
      { status: 503 }
    );
  }
  await listener.tick(); // guardedTick: watchdog-capped, all applies idempotent
  return Response.json({ ticked: true }, { headers: { 'cache-control': 'no-store' } });
}
