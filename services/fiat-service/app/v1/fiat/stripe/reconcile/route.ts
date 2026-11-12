// TX1 §8 — POST /v1/fiat/stripe/reconcile: reconcile Stripe against the
// journal, now. Kicked once a day by cron (like /settlement/tick is once a
// minute); every finding goes to the service journal as an ALARM line, where
// the existing alarm mail picks it up, and comes back in the answer.
// Server-to-server only: guarded by FIAT_ADMIN_TOKEN, deliberately NOT in the
// middleware's browser-CORS route set.
import { getFiatDeps } from '../../../../../src/lib/fiat-session-service';
import { productionLegRecovery, productionStripeReconcile, reconcileStripe } from '../../../../../src/lib/fiat-reconcile';
import { recoverPendingCashoutLegs } from '../../../../../src/lib/fiat-cashout-service';

export async function POST(req: Request): Promise<Response> {
  const token = process.env.FIAT_ADMIN_TOKEN;
  if (!token) {
    return Response.json({ error: 'FIAT_ADMIN_TOKEN is not set — reconcile endpoint disabled' }, { status: 503 });
  }
  if ((req.headers.get('authorization') ?? '') !== `Bearer ${token}`) {
    return Response.json({ error: 'unauthorised' }, { status: 401 });
  }
  let report;
  let recovery;
  try {
    const { ledger } = await getFiatDeps();
    // First settle refund legs left pending (a crash between the debit and
    // Stripe's answer): confirm, complete or reverse each. What is still
    // pending afterwards is reported by the comparison below.
    recovery = await recoverPendingCashoutLegs(productionLegRecovery(ledger));
    report = await reconcileStripe(productionStripeReconcile(ledger));
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    console.error(`[fiat-reconcile] ALARM: stripe reconciliation did not run: ${message}`);
    return Response.json({ error: 'reconcile_failed', message }, { status: 502 });
  }
  return Response.json(
    {
      sessions: report.sessions,
      refunds: report.refunds,
      disputes: report.disputes,
      receivableMicro: report.receivableMicro.toString(),
      pendingLegs: recovery,
      findings: report.findings,
    },
    { headers: { 'cache-control': 'no-store' } }
  );
}
