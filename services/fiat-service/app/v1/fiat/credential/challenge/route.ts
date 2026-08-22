// FC2.2 — GET /v1/fiat/credential/challenge?address=0x…
//
// Step (a) of self-serve minting (Decision 1): issue a fresh, single-use,
// server-remembered challenge for the address. Unauthenticated by design —
// anyone may request a challenge, but it is worthless without a signature the
// on-chain check accepts, and the store is DoS-bounded (Decision 9: one nonce
// per address, 5-min TTL, global cap → 429). Returns BOTH a human-legible string
// (personal_sign) and the equivalent EIP-712 payload (typed data) so the browser
// can sign whichever the wallet supports.
import { fiatUserId } from '../../../../../src/lib/fiat-identity';
import {
  ChallengeStoreFullError,
  buildChallengeTypedData,
  getChallengeStore,
  parseIntentName,
} from '../../../../../src/lib/fiat-challenge';

export async function GET(req: Request): Promise<Response> {
  const params = new URL(req.url).searchParams;
  const address = params.get('address');
  let addr: string;
  try {
    addr = fiatUserId(address ?? '');
  } catch {
    return Response.json({ error: 'address query param must be a valid address' }, { status: 400 });
  }

  // Optional `intent`: which wording the user will be asked to sign. Clients
  // that verify the message character-for-character opt into the newer wording
  // when they are ready; omitting it keeps today's string, so no client has to
  // deploy in step with the service. Unknown values are refused rather than
  // echoed — arbitrary text in a signing prompt would be a phishing tool.
  const intent = parseIntentName(params.get('intent'));
  if (!intent) {
    return Response.json(
      { error: 'intent, when present, must be one of: rendering, compute' },
      { status: 400 }
    );
  }

  let challenge;
  try {
    challenge = getChallengeStore().issue(addr, intent);
  } catch (e) {
    if (e instanceof ChallengeStoreFullError) return Response.json({ error: e.message }, { status: 429 });
    throw e;
  }

  // The nonce is single-use and money-adjacent — never cache it.
  return Response.json(
    {
      nonce: challenge.nonce,
      message: challenge.message,
      typedData: buildChallengeTypedData(challenge),
      expiresAt: challenge.expiresAt,
      // Echoed so a client can assert it got the wording it asked for.
      intent: challenge.intent,
    },
    { headers: { 'cache-control': 'no-store' } }
  );
}
