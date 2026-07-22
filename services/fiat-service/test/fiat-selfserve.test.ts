// FC2.2 — self-serve credential minting: GET challenge + POST self-serve, the
// ChallengeStore (Decision 9 bounds), and Decision-8 keep-newest-per-purpose seen
// through the routes. Signature verification is injected (FC2.1 has its own
// tests); here we prove the nonce lifecycle, the address binding, and the mint.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { GET as challengeGET } from '../app/v1/fiat/credential/challenge/route';
import { POST as selfServePOST } from '../app/v1/fiat/credential/self-serve/route';
import { ChallengeStore, setChallengeStoreForTest } from '../src/lib/fiat-challenge';
import { setSignatureVerifierForTest, SignatureCheckUnavailableError, type OwnershipProof } from '../src/lib/fiat-signature';
import { setFiatDepsForTest } from '../src/lib/fiat-session-service';
import { FiatCredentials } from '../src/lib/fiat-credentials';
import { MemoryLedgerStore } from '../src/lib/ledger';

const ADDR = '0xAbCdef0123456789abcDEF0123456789aBCdEf01';
const ADDR_LC = ADDR.toLowerCase();
const ADDR_B = '0x1111111111111111111111111111111111111111';

let credentials: FiatCredentials;

/** A fake verifier: a signature is "good" iff it is the good one for the address
 *  the SERVER passed in (proof.address), regardless of message or typed-data. */
const goodSig = (address: string) => `GOOD:${address.toLowerCase()}`;
const acceptGoodSig = async (proof: OwnershipProof) => proof.signature === goodSig(proof.address);

beforeEach(async () => {
  credentials = await FiatCredentials.open(new MemoryLedgerStore());
  setFiatDepsForTest({ credentials });
  setChallengeStoreForTest(new ChallengeStore()); // real defaults unless a test overrides
  setSignatureVerifierForTest(acceptGoodSig);
});

afterEach(() => {
  setFiatDepsForTest(undefined);
  setChallengeStoreForTest(undefined);
  setSignatureVerifierForTest(undefined);
});

function getChallenge(address: string) {
  return challengeGET(new Request(`http://site/api/fiat/credential/challenge?address=${address}`));
}
function postSelfServe(body: unknown) {
  return selfServePOST(
    new Request('http://site/api/fiat/credential/self-serve', { method: 'POST', body: JSON.stringify(body) })
  );
}
async function issuedNonce(address: string): Promise<{ nonce: string; message: string; typedData: unknown; expiresAt: number }> {
  const res = await getChallenge(address);
  expect(res.status).toBe(200);
  return res.json();
}

describe('GET /api/fiat/credential/challenge', () => {
  it('issues a nonce + a human-legible message binding the intent, lowercased address, nonce and expiry', async () => {
    const c = await issuedNonce(ADDR);
    expect(c.nonce).toMatch(/^[0-9a-f]{48}$/);
    expect(c.message).toContain('Platformless AI');
    expect(c.message).toContain(`address: ${ADDR_LC}`);
    expect(c.message).toContain(`nonce: ${c.nonce}`);
    expect(c.message).toContain('expires:');
    expect(c.typedData).toBeDefined();
    expect(c.expiresAt).toBeGreaterThan(0);
  });

  it('400s on a malformed address', async () => {
    expect((await getChallenge('nope')).status).toBe(400);
    expect((await challengeGET(new Request('http://site/api/fiat/credential/challenge'))).status).toBe(400);
  });

  it('429s once the global outstanding-challenge cap is hit', async () => {
    setChallengeStoreForTest(new ChallengeStore(Date.now, 5 * 60_000, 1)); // cap = 1
    expect((await getChallenge(ADDR)).status).toBe(200);
    expect((await getChallenge(ADDR_B)).status).toBe(429);
  });

  it('a second challenge for the same address returns the SAME live nonce (idempotent — no eviction)', async () => {
    // Prevents a targeted grief: an attacker who knows a victim's public address
    // must not be able to evict the victim's in-flight nonce by spamming this
    // endpoint during their passkey prompt.
    const first = await issuedNonce(ADDR);
    const second = await issuedNonce(ADDR);
    expect(second.nonce).toBe(first.nonce); // handed back, not replaced
    const res = await postSelfServe({ nonce: first.nonce, signature: goodSig(ADDR), purpose: 'helper' });
    expect(res.status).toBe(200); // still mints — was not evicted
  });
});

describe('POST /api/fiat/credential/self-serve — the mint', () => {
  it('verifies, consumes the nonce, and returns a credential that authenticates as the lowercased address', async () => {
    const c = await issuedNonce(ADDR);
    const res = await postSelfServe({ nonce: c.nonce, signature: goodSig(ADDR), purpose: 'helper' });
    expect(res.status).toBe(200);
    const { credential } = await res.json();
    expect(credentials.authenticate(credential)).toBe(ADDR_LC);
  });

  it('mints via the typed-data method path, dispatching a TYPED-DATA proof (not a string message)', async () => {
    let sawTypedData = false;
    setSignatureVerifierForTest(async (proof) => {
      sawTypedData = 'typedData' in proof && !('message' in proof);
      return proof.signature === goodSig(proof.address);
    });
    const c = await issuedNonce(ADDR);
    const res = await postSelfServe({ nonce: c.nonce, signature: goodSig(ADDR), purpose: 'browser', method: 'typed' });
    expect(res.status).toBe(200);
    expect(sawTypedData).toBe(true); // the route built a typed-data proof, not a message proof
    const { credential } = await res.json();
    expect(credentials.authenticate(credential)).toBe(ADDR_LC);
  });

  it('a replayed nonce is rejected (single-use)', async () => {
    const c = await issuedNonce(ADDR);
    expect((await postSelfServe({ nonce: c.nonce, signature: goodSig(ADDR), purpose: 'helper' })).status).toBe(200);
    expect((await postSelfServe({ nonce: c.nonce, signature: goodSig(ADDR), purpose: 'helper' })).status).toBe(401);
  });

  it('an expired nonce is rejected', async () => {
    let clock = 1_000;
    setChallengeStoreForTest(new ChallengeStore(() => clock, 1_000, 10_000)); // 1s TTL
    const c = await issuedNonce(ADDR);
    clock += 5_000; // past expiry
    expect((await postSelfServe({ nonce: c.nonce, signature: goodSig(ADDR), purpose: 'helper' })).status).toBe(401);
  });

  it('a FAILED verify still consumes the nonce (a retry needs a fresh challenge)', async () => {
    const c = await issuedNonce(ADDR);
    expect((await postSelfServe({ nonce: c.nonce, signature: 'BAD', purpose: 'helper' })).status).toBe(401);
    // same nonce, now with a GOOD sig — still rejected, because it was burned.
    expect((await postSelfServe({ nonce: c.nonce, signature: goodSig(ADDR), purpose: 'helper' })).status).toBe(401);
  });

  it('a wrong-signer signature is rejected (401), nothing minted', async () => {
    const c = await issuedNonce(ADDR);
    // A signature that is "good" for B, presented against A's server-stored address.
    const res = await postSelfServe({ nonce: c.nonce, signature: goodSig(ADDR_B), purpose: 'helper' });
    expect(res.status).toBe(401);
  });

  it("A's nonce cannot mint for B: the credential is ALWAYS for the server-stored address", async () => {
    const c = await issuedNonce(ADDR);
    // The body has no address field; even a valid A-signature only ever mints A.
    const res = await postSelfServe({ nonce: c.nonce, signature: goodSig(ADDR), purpose: 'helper' });
    const { credential } = await res.json();
    expect(credentials.authenticate(credential)).toBe(ADDR_LC);
    expect(credentials.authenticate(credential)).not.toBe(ADDR_B);
  });

  it('an unknown purpose → 400 (and does not consume the nonce)', async () => {
    const c = await issuedNonce(ADDR);
    expect((await postSelfServe({ nonce: c.nonce, signature: goodSig(ADDR), purpose: 'admin' })).status).toBe(400);
    // nonce not burned — a valid attempt still works
    expect((await postSelfServe({ nonce: c.nonce, signature: goodSig(ADDR), purpose: 'helper' })).status).toBe(200);
  });

  it('a MISSING purpose → 400 (no dangerous default to helper)', async () => {
    const c = await issuedNonce(ADDR);
    expect((await postSelfServe({ nonce: c.nonce, signature: goodSig(ADDR) })).status).toBe(400);
  });

  it('an unknown method and missing fields → 400', async () => {
    const c = await issuedNonce(ADDR);
    expect((await postSelfServe({ nonce: c.nonce, signature: goodSig(ADDR), method: 'weird' })).status).toBe(400);
    expect((await postSelfServe({ signature: goodSig(ADDR) })).status).toBe(400); // no nonce
    expect((await postSelfServe({ nonce: c.nonce })).status).toBe(400); // no signature
  });

  it('an RPC outage during verification → 503, and the nonce is burned', async () => {
    setSignatureVerifierForTest(async () => {
      throw new SignatureCheckUnavailableError();
    });
    const c = await issuedNonce(ADDR);
    expect((await postSelfServe({ nonce: c.nonce, signature: goodSig(ADDR), purpose: 'helper' })).status).toBe(503);
    // burned — even a working verifier can't reuse it
    setSignatureVerifierForTest(acceptGoodSig);
    expect((await postSelfServe({ nonce: c.nonce, signature: goodSig(ADDR), purpose: 'helper' })).status).toBe(401);
  });
});

describe('Decision 8 keep-newest-per-purpose, through the routes', () => {
  async function mint(purpose: 'helper' | 'browser'): Promise<string> {
    const c = await issuedNonce(ADDR);
    const res = await postSelfServe({ nonce: c.nonce, signature: goodSig(ADDR), purpose });
    expect(res.status).toBe(200);
    return (await res.json()).credential;
  }

  it('a second browser mint evicts the first browser credential but NEVER the helper one', async () => {
    const helper = await mint('helper');
    const browser1 = await mint('browser');
    const browser2 = await mint('browser');
    expect(credentials.authenticate(helper)).toBe(ADDR_LC); // helper survives browser churn
    expect(credentials.authenticate(browser1)).toBeNull(); // old browser evicted
    expect(credentials.authenticate(browser2)).toBe(ADDR_LC); // newest browser alive
  });
});
