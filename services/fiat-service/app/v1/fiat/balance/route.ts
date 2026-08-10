// FC1 — GET /v1/fiat/balance?address=0x…: the fiat-credits balance for an
// address, for the account page to display. A read of a display value keyed on
// the (pseudonymous) address; no auth at launch (spending needs a credential,
// which is the real boundary). Returns micro-USDC as a string — the UI formats
// it with lib/credits.ts `formatCreditsFromMicro`.
import { getFiatDeps } from '../../../../src/lib/fiat-session-service';
import { fiatUserId } from '../../../../src/lib/fiat-identity';

export async function GET(req: Request): Promise<Response> {
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
    return Response.json(
      { error: e instanceof Error ? e.message : 'fiat backend unavailable' },
      { status: 503 }
    );
  }

  return Response.json({ availableMicro: ledger.availableMicro(userId).toString() });
}
