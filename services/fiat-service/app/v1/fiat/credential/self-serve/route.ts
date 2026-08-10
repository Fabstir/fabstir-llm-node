// FC2.2 — POST /v1/fiat/credential/self-serve { nonce, signature, purpose, method }
//
// Step (c) of self-serve minting (Decision 1), the security boundary of the whole
// milestone: mint a SPENDING credential only after an on-chain-verified proof that
// the caller controls the challenge's address.
//
//   1. Validate the request shape (missing/unknown purpose or method → 400).
//   2. CONSUME the nonce (Decision 9) BEFORE verifying — a failed verify still
//      burns it, so a captured nonce can never be brute-forced or replayed.
//   3. Verify the signature (FC2.1) against the address + message the SERVER
//      stored for that nonce — never a client-supplied address or message. That
//      binding is what makes ownership meaningful; viem handles EOA/EIP-1271/
//      ERC-6492 (the undeployed smart-account case).
//   4. Only then mint, for the stored address, under the requested purpose.
//
// An RPC outage during verification is a 503 (the ownership question is
// unanswered — retry with a fresh challenge), never a silent "not the owner".
import { getFiatDeps } from '../../../../../src/lib/fiat-session-service';
import { buildChallengeTypedData, getChallengeStore } from '../../../../../src/lib/fiat-challenge';
import {
  SignatureCheckUnavailableError,
  getSignatureVerifier,
  type OwnershipProof,
} from '../../../../../src/lib/fiat-signature';
import type { CredentialPurpose } from '../../../../../src/lib/fiat-credentials';

const PURPOSES = new Set<CredentialPurpose>(['helper', 'browser']);

export async function POST(req: Request): Promise<Response> {
  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return Response.json({ error: 'body must be JSON' }, { status: 400 });
  }

  const b = (body ?? {}) as { nonce?: unknown; signature?: unknown; purpose?: unknown; method?: string };
  const { nonce, signature, purpose } = b;
  const method = b.method ?? 'personal_sign';

  // Purpose is REQUIRED — no default. A defaulted 'helper' would let a bare API
  // caller (or a wiring slip) mint, and thereby rotate, the helper's live
  // credential without intending to. The browser client always sends it.
  if (typeof purpose !== 'string' || !PURPOSES.has(purpose as CredentialPurpose)) {
    return Response.json({ error: "purpose must be 'helper' or 'browser'" }, { status: 400 });
  }
  if (method !== 'personal_sign' && method !== 'typed') {
    return Response.json({ error: "method must be 'personal_sign' or 'typed'" }, { status: 400 });
  }
  if (typeof nonce !== 'string' || nonce.length === 0) {
    return Response.json({ error: 'nonce is required' }, { status: 400 });
  }
  if (typeof signature !== 'string' || signature.length === 0) {
    return Response.json({ error: 'signature is required' }, { status: 400 });
  }

  // Resolve the backend BEFORE consuming the nonce, so a fiat-backend-down 503
  // doesn't waste the user's passkey signature + nonce (fail fast). Consume still
  // happens before verify, so the security ordering (Decision 9) is unchanged.
  let credentials;
  try {
    ({ credentials } = await getFiatDeps());
  } catch {
    // Any config detail (env-var names, paths) stays server-side; generic 503.
    return Response.json({ error: 'fiat backend unavailable' }, { status: 503 });
  }

  // Decision 9: consume the nonce on ANY attempt that reaches verification.
  const challenge = getChallengeStore().consume(nonce);
  if (!challenge) {
    return Response.json({ error: 'challenge unknown, expired, or already used' }, { status: 401 });
  }

  // Verify over the SERVER-STORED address + payload, never a client-supplied one.
  const proof: OwnershipProof =
    method === 'typed'
      ? { address: challenge.address, typedData: buildChallengeTypedData(challenge), signature }
      : { address: challenge.address, message: challenge.message, signature };

  let verified: boolean;
  try {
    verified = await getSignatureVerifier()(proof);
  } catch (e) {
    if (e instanceof SignatureCheckUnavailableError) {
      return Response.json(
        { error: 'signature verification temporarily unavailable — request a new challenge and retry' },
        { status: 503 }
      );
    }
    // Malformed signature / bad input — a failed proof (the nonce is already burned).
    return Response.json({ error: 'signature did not verify' }, { status: 401 });
  }
  if (!verified) {
    return Response.json({ error: 'signature did not verify' }, { status: 401 });
  }

  const credential = await credentials.issue(challenge.address, purpose as CredentialPurpose);
  // The credential is a spending secret — never let a cache retain it.
  return Response.json({ credential }, { headers: { 'cache-control': 'no-store' } });
}
