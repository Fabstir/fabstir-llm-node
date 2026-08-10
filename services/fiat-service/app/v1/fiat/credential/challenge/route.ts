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
} from '../../../../../src/lib/fiat-challenge';

export async function GET(req: Request): Promise<Response> {
  const address = new URL(req.url).searchParams.get('address');
  let addr: string;
  try {
    addr = fiatUserId(address ?? '');
  } catch {
    return Response.json({ error: 'address query param must be a valid address' }, { status: 400 });
  }

  let challenge;
  try {
    challenge = getChallengeStore().issue(addr);
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
    },
    { headers: { 'cache-control': 'no-store' } }
  );
}
