// LIVE UI-walk harness — plays the USER's role against the deployed service,
// exactly as the browser client does, with a fresh throwaway EOA generated per
// run. No mocks, no server access, no human. Stages:
//   A. read-only surface: health of every public endpoint + CORS + armed gates
//   B. the credential walk: challenge → local EIP-191 sign → self-serve mint →
//      authenticated call — the same path a passkey user takes (the verifier
//      accepts plain EOA signatures alongside 1271/6492)
//   C. (separate runner) purchase via Stripe test event → webhook → balance
//
// Inert unless LIVE_UI_WALK=1:
//   LIVE_UI_WALK=1 npx vitest run --no-file-parallelism test/live-ui-walk-harness.test.ts
import { describe, expect, it } from 'vitest';
import { Wallet } from 'ethers';

const BASE = process.env.LIVE_FIAT_BASE ?? 'https://fiat.fabstir.net/v1/fiat';

async function j(res: Response): Promise<Record<string, unknown>> {
  return (await res.json()) as Record<string, unknown>;
}

describe.runIf(process.env.LIVE_UI_WALK === '1')('LIVE UI walk against the deployed service', () => {
  const user = Wallet.createRandom();
  console.log(`[walk] throwaway user for this run: ${user.address}`);

  it('A: public surface — balance, CORS preflight, armed session-auth, guarded tick', async () => {
    // A1: a brand-new address reads a zero balance (and the route answers)
    const bal = await fetch(`${BASE}/balance?address=${user.address}`);
    expect(bal.status).toBe(200);
    expect((await j(bal)).availableMicro).toBe('0');
    console.log('[walk] A1 OK — balance route live, fresh user reads 0');

    // A2: CORS preflight for the allowed dev origin
    const pre = await fetch(`${BASE}/balance`, {
      method: 'OPTIONS',
      headers: { origin: 'http://localhost:3022', 'access-control-request-method': 'GET' },
    });
    expect(pre.status).toBe(204);
    expect(pre.headers.get('access-control-allow-origin')).toBe('http://localhost:3022');
    console.log('[walk] A2 OK — CORS pinned and answering');

    // A3: the render host's FC1.6 gate is armed (wrong sig → 401, not 404/503)
    const auth = await fetch('https://host1.fabstir.net/v1/session-auth', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        sessionId: '1',
        clientAddress: user.address,
        scheme: 'fc1-session-auth-v1',
        signature: '0xdeadbeef',
      }),
    });
    expect(auth.status).toBe(401);
    console.log('[walk] A3 OK — node session-auth gate armed (401 on garbage signature)');

    // A4: the settlement tick endpoint refuses the unauthenticated world
    const tick = await fetch(`${BASE}/settlement/tick`, { method: 'POST' });
    expect(tick.status).toBe(401);
    console.log('[walk] A4 OK — external tick endpoint exists and is token-guarded');

    // A5: both health paths on the render host answer identically
    const h1 = await (await fetch('https://host1.fabstir.net/health')).text();
    const h2 = await (await fetch('https://host1.fabstir.net/v1/health')).text();
    expect(h1).toBe(h2);
    console.log('[walk] A5 OK — /health and /v1/health identical on the render host');
  }, 60_000);

  it('B: the full credential walk as a brand-new user (challenge → sign → mint → use)', async () => {
    // B1: challenge
    const ch = await fetch(`${BASE}/credential/challenge?address=${user.address}`);
    expect(ch.status).toBe(200);
    const challenge = (await j(ch)) as { nonce: string; message: string };
    expect(challenge.nonce.length).toBeGreaterThan(20);
    expect(challenge.message).toContain(user.address.toLowerCase().replace(/^0x/, '0x'));
    console.log(`[walk] B1 OK — challenge issued, nonce ${challenge.nonce.slice(0, 8)}…`);

    // B2: sign locally — EIP-191 personal_sign, the same op a wallet performs
    const signature = await user.signMessage(challenge.message);

    // B3: mint — the server verifies ownership ON-CHAIN-COMPATIBLY (EOA path here)
    const mint = await fetch(`${BASE}/credential/self-serve`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ nonce: challenge.nonce, signature, purpose: 'browser' }),
    });
    const mintBody = await j(mint);
    expect(mint.status, JSON.stringify(mintBody)).toBe(200);
    const credential = mintBody.credential as string;
    expect(credential.length).toBeGreaterThan(20);
    console.log('[walk] B3 OK — credential MINTED via live signature verification');

    // B4: the credential authenticates — a cash-out attempt must fail on FUNDS,
    // not on auth (a fresh user holds 0; the distinction proves the credential
    // is accepted and the money gate is doing its own job)
    const co = await fetch(`${BASE}/cashout`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', authorization: `Bearer ${credential}` },
      body: JSON.stringify({ amountMicro: '500000' }),
    });
    const coBody = await j(co);
    expect(co.status, JSON.stringify(coBody)).not.toBe(401); // 401 = credential rejected = FAIL
    expect([400, 402, 403]).toContain(co.status); // refused for balance, as it must be
    console.log(`[walk] B4 OK — credential authenticates; cash-out correctly refused for funds (${co.status})`);

    // B5: replay protection — the consumed nonce must be dead
    const replay = await fetch(`${BASE}/credential/self-serve`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ nonce: challenge.nonce, signature, purpose: 'browser' }),
    });
    expect(replay.status).toBe(401);
    console.log('[walk] B5 OK — nonce single-use enforced (replay rejected)');
  }, 60_000);
});
