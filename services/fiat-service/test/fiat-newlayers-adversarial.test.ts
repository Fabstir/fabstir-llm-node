// FC1 fiat money-path — ADVERSARIAL PASS #2 (the NEWER layers on top of the
// pass-#1 ledger/gatekeeper core in fiat-ledger-adversarial.test.ts).
//
// Scope: the vault signer, credentials, session-open orchestration + route,
// Stripe webhook/verify/extract, cash-out service + route, and the settlement
// listener. Each block probes an attack the existing fiat-*.test.ts files do
// NOT already cover. Threat model: a bypass that opens a vault-paid session
// without a valid credential+gatekeeper pass is theft; a webhook forgery mints
// money; a cash-out trick converts credits to cash beyond the card charge.
//
// One block (F1) DEMONSTRATES A LIVE VULNERABILITY and is labelled loudly.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { createHmac } from 'node:crypto';

import { POST as sessionRoute } from '../app/v1/fiat/session/route';
import { POST as webhookRoute } from '../app/v1/fiat/stripe/webhook/route';
import { POST as credentialRoute } from '../app/v1/fiat/credential/route';
import {
  openFiatSession,
  setFiatDepsForTest,
  setFiatSessionServiceForTest,
  type FiatSessionDeps,
} from '../src/lib/fiat-session-service';
import { requestCashout, type CashoutDeps } from '../src/lib/fiat-cashout-service';
import { extractPurchase, verifyStripeSignature } from '../src/lib/stripe';
import { applySettlementEvents, type SettlementEvent } from '../src/lib/settlement-listener';
import { CreditsLedger, MemoryLedgerStore } from '../src/lib/ledger';
import { FiatCredentials } from '../src/lib/fiat-credentials';
import { makeGatekeeper, type Gatekeeper } from '../src/lib/gatekeeper';
import type { SessionAuthorisation } from '../src/lib/fiat-vault';

const HOST = '0xabcd000000000000000000000000000000000001';
const OFFLIST = '0x9999999999999999999999999999999999999999';
const MODEL = `0x${'ab'.repeat(32)}`;
const CLIENT = '0x1234567890abcdef1234567890abcdef12345678';
const VAULT = '0x8ba1f109551bD432803012645Ac136ddd64DBA72';

const gate: Gatekeeper = makeGatekeeper({
  allowedHosts: [HOST],
  maxDepositPerSessionMicro: 2_000_000n,
  maxDailySpendMicro: 10_000_000n,
  maxOpensPerMinute: 10,
});

const fakeSignAuth = (sessionId: bigint, clientAddress: string): SessionAuthorisation => ({
  scheme: 'fc1-session-auth-v1',
  signature: `0xsig-${sessionId}-${clientAddress}`,
  clientAddress,
});

// ---------------------------------------------------------------------------
// Session route: header + body smuggling the existing route test does not cover
// ---------------------------------------------------------------------------
describe('session route: header and body smuggling', () => {
  const GOOD = { host: HOST, modelId: MODEL, depositMicro: '500000', clientAddress: CLIENT };

  function postRaw(headers: Record<string, string>, body: unknown) {
    return sessionRoute(
      new Request('http://site/api/fiat/session', {
        method: 'POST',
        headers,
        body: typeof body === 'string' ? body : JSON.stringify(body),
      })
    );
  }

  // A stub that records exactly what the route forwarded, so we can prove which
  // requests reached the service and with what (already-parsed) values.
  function recordingStub() {
    const seen: unknown[] = [];
    setFiatSessionServiceForTest({
      open: async (req) => {
        seen.push(req);
        return { status: 'unauthorised' };
      },
    });
    return seen;
  }
  afterEach(() => setFiatSessionServiceForTest(undefined));

  it('lower-case "bearer " scheme is rejected (case-sensitive prefix), service untouched', async () => {
    const seen = recordingStub();
    const res = await postRaw({ authorization: 'bearer fc1_x' }, GOOD);
    expect(res.status).toBe(401);
    expect(seen).toHaveLength(0);
  });

  it('a lone "Bearer " (no token) is 401 at the route: the header value is trimmed to "Bearer", failing the scheme prefix', async () => {
    const seen = recordingStub();
    // HTTP header values are stripped of trailing whitespace by the Request/
    // Headers layer, so "Bearer " -> "Bearer", which does NOT startWith
    // "Bearer " -> 401 before the service. An empty credential never reaches
    // the gatekeeper this way; the route fails closed.
    const res = await postRaw({ authorization: 'Bearer ' }, GOOD);
    expect(res.status).toBe(401);
    expect(seen).toHaveLength(0);
  });

  it('a second space is part of the token (slice(7)), never trimmed — a mangled token must not authenticate', async () => {
    const seen = recordingStub();
    await postRaw({ authorization: 'Bearer  fc1_real' }, GOOD); // two spaces
    expect(seen).toEqual([
      { credential: ' fc1_real', host: HOST, modelId: MODEL, depositMicro: 500_000n, clientAddress: CLIENT },
    ]);
  });

  it('capitalised Authorization header name still works (HTTP header names are case-insensitive)', async () => {
    const seen = recordingStub();
    const res = await postRaw({ Authorization: 'Bearer fc1_x' }, GOOD);
    expect(res.status).toBe(401); // stub -> unauthorised, but the request DID reach the service
    expect(seen).toHaveLength(1);
  });

  it('prototype-pollution keys in the body are inert and never reach the service as fields', async () => {
    const seen = recordingStub();
    // __proto__/constructor keys must neither pollute Object.prototype nor be
    // read (the route destructures only host/modelId/depositMicro/clientAddress).
    await postRaw(
      { authorization: 'Bearer fc1_x' },
      { ...GOOD, __proto__: { polluted: 1 }, constructor: { prototype: { polluted2: 1 } }, extra: 'ignored' }
    );
    expect(({} as Record<string, unknown>).polluted).toBeUndefined();
    expect(({} as Record<string, unknown>).polluted2).toBeUndefined();
    expect(seen).toEqual([
      { credential: 'fc1_x', host: HOST, modelId: MODEL, depositMicro: 500_000n, clientAddress: CLIENT },
    ]);
  });

  it('arrays and objects where strings are expected are 400s (typeof guard), service untouched', async () => {
    const seen = recordingStub();
    expect((await postRaw({ authorization: 'Bearer x' }, { ...GOOD, host: [HOST] })).status).toBe(400);
    expect((await postRaw({ authorization: 'Bearer x' }, { ...GOOD, modelId: { toString: () => MODEL } })).status).toBe(400);
    expect((await postRaw({ authorization: 'Bearer x' }, { ...GOOD, clientAddress: [CLIENT] })).status).toBe(400);
    expect((await postRaw({ authorization: 'Bearer x' }, { ...GOOD, depositMicro: ['500000'] })).status).toBe(400);
    expect(seen).toHaveLength(0);
  });

  it('trailing newline / CRLF in modelId or depositMicro is rejected (regexes are end-anchored)', async () => {
    const seen = recordingStub();
    // JS "$" (no m flag) does NOT match before a trailing \n, so these are 400s.
    expect((await postRaw({ authorization: 'Bearer x' }, { ...GOOD, modelId: `${MODEL}\n` })).status).toBe(400);
    expect((await postRaw({ authorization: 'Bearer x' }, { ...GOOD, depositMicro: '500000\n' })).status).toBe(400);
    expect((await postRaw({ authorization: 'Bearer x' }, { ...GOOD, depositMicro: '500000\r\n' })).status).toBe(400);
    expect((await postRaw({ authorization: 'Bearer x' }, { ...GOOD, depositMicro: ' 500000' })).status).toBe(400);
    expect((await postRaw({ authorization: 'Bearer x' }, { ...GOOD, depositMicro: '0x10' })).status).toBe(400);
    expect(seen).toHaveLength(0);
  });
});

describe('session route -> service: depositMicro "0" is not blocked at the route but refused by the gatekeeper', () => {
  afterEach(() => setFiatSessionServiceForTest(undefined));

  it('"0" passes /^\\d+$/, becomes 0n, and the gatekeeper returns INVALID_DEPOSIT (403), never a session', async () => {
    // Wire a REAL service (real ledger + gatekeeper) so we prove the end-to-end
    // outcome, not just that the route forwards it.
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    const credentials = await FiatCredentials.open(new MemoryLedgerStore());
    const token = await credentials.issue('user-1');
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const deps: FiatSessionDeps = {
      ledger,
      credentials,
      gatekeeper: gate,
      chain: {
        ensureAllowance: async () => {
          throw new Error('chain must never be touched for a 0 deposit');
        },
        createSession: async () => {
          throw new Error('chain must never be touched for a 0 deposit');
        },
      },
      signAuth: fakeSignAuth,
    };
    setFiatSessionServiceForTest({ open: (req) => openFiatSession(deps, req) });

    const res = await sessionRoute(
      new Request('http://site/api/fiat/session', {
        method: 'POST',
        headers: { authorization: `Bearer ${token}` },
        body: JSON.stringify({ host: HOST, modelId: MODEL, depositMicro: '0', clientAddress: CLIENT }),
      })
    );
    expect(res.status).toBe(403);
    expect(await res.json()).toEqual({ error: 'refused', reason: 'INVALID_DEPOSIT' });
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n); // untouched
  });
});

// ---------------------------------------------------------------------------
// openFiatSession: the loud divergence path — create succeeds, bind CANNOT.
// The comment in the source promises this "escapes loudly — never a silent
// release". Nothing tests it. A silent release here would let the user re-spend
// money the vault has ALREADY paid on-chain.
// ---------------------------------------------------------------------------
describe('openFiatSession: a create that cannot be bound must throw and keep the hold (no silent release)', () => {
  it('a jobId collision at bind time propagates and the users money stays held, not refunded', async () => {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    const credentials = await FiatCredentials.open(new MemoryLedgerStore());
    const token = await credentials.issue('user-1');
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');

    // Pre-bind jobId 842 to a DECOY hold, so the real bind collides.
    await ledger.purchase('decoy', 1_000_000n, 'evt_decoy');
    const decoy = await ledger.openHold({ userId: 'decoy', host: HOST, depositMicro: 100_000n }, gate);
    if (!decoy.ok) throw new Error('decoy open refused');
    await ledger.bindSession(decoy.holdId, 842n);

    const deps: FiatSessionDeps = {
      ledger,
      credentials,
      gatekeeper: gate,
      chain: {
        ensureAllowance: async () => {},
        createSession: async () => ({ jobId: 842n, depositor: VAULT, txHash: '0xtx' }), // collides with decoy
      },
      signAuth: fakeSignAuth,
    };

    await expect(
      openFiatSession(deps, { credential: token, host: HOST, modelId: MODEL, depositMicro: 500_000n, clientAddress: CLIENT })
    ).rejects.toThrow(/already bound/i);

    // The security property: the hold is NOT released. The user paid on-chain,
    // so their credits must stay debited pending human reconciliation.
    expect(ledger.availableMicro('user-1')).toBe(500_000n); // 1,000,000 - 500,000 held (NOT restored)
    expect(ledger.userForJob(842n)).toBe('decoy'); // mapping never hijacked
    expect(ledger.outstandingMicro()).toBeGreaterThanOrEqual(500_000n);
  });
});

// ---------------------------------------------------------------------------
// Credentials: revoke/reissue semantics and near-miss / non-string inputs
// ---------------------------------------------------------------------------
describe('FiatCredentials: revoke-then-reissue and hostile authenticate inputs', () => {
  it('a revoked token never comes back to life after the user is reissued a fresh token', async () => {
    const creds = await FiatCredentials.open(new MemoryLedgerStore());
    const oldToken = await creds.issue('user-1');
    expect(await creds.revokeAll('user-1')).toBe(1);
    const newToken = await creds.issue('user-1');
    expect(newToken).not.toBe(oldToken);
    expect(creds.authenticate(oldToken)).toBeNull(); // stays dead
    expect(creds.authenticate(newToken)).toBe('user-1'); // fresh one works
  });

  it('a correct-shape near-miss token (right length/prefix, wrong bytes) does not authenticate', async () => {
    const creds = await FiatCredentials.open(new MemoryLedgerStore());
    const real = await creds.issue('user-1');
    const nearMiss = real.slice(0, -1) + (real.endsWith('0') ? '1' : '0'); // flip last hex nibble
    expect(nearMiss).not.toBe(real);
    expect(nearMiss).toMatch(/^fc1_[0-9a-f]{64}$/);
    expect(creds.authenticate(nearMiss)).toBeNull();
  });

  it('non-string tokens are refused defensively (no hashing of objects/null/arrays)', async () => {
    const creds = await FiatCredentials.open(new MemoryLedgerStore());
    await creds.issue('user-1');
    expect(creds.authenticate(null as unknown as string)).toBeNull();
    expect(creds.authenticate(undefined as unknown as string)).toBeNull();
    expect(creds.authenticate({} as unknown as string)).toBeNull();
    expect(creds.authenticate(['fc1_x'] as unknown as string)).toBeNull();
    expect(creds.authenticate(12345 as unknown as string)).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Credential issuance route: operator-token comparison edges
// ---------------------------------------------------------------------------
describe('credential issuance route: operator token comparison', () => {
  let credentials: FiatCredentials;
  beforeEach(async () => {
    credentials = await FiatCredentials.open(new MemoryLedgerStore());
    setFiatDepsForTest({ credentials });
  });
  afterEach(() => {
    delete process.env.FIAT_ADMIN_TOKEN;
    setFiatDepsForTest(undefined);
  });

  function post(token: string | undefined, body: unknown = { userId: '0x1234567890abcdef1234567890abcdef12345678' }) {
    return credentialRoute(
      new Request('http://site/api/fiat/credential', {
        method: 'POST',
        headers: token === undefined ? {} : { authorization: `Bearer ${token}` },
        body: JSON.stringify(body),
      })
    );
  }

  it('a token that is a PREFIX of the admin secret is rejected (hash compare, not prefix/length)', async () => {
    process.env.FIAT_ADMIN_TOKEN = 'admin-secret-long';
    expect((await post('admin-secret')).status).toBe(401); // prefix
    expect((await post('admin-secret-longer')).status).toBe(401); // superstring
    expect((await post('admin-secret-long')).status).toBe(200); // exact
  });

  it('an empty admin token env disables the endpoint (503), even when the caller sends an empty bearer', async () => {
    process.env.FIAT_ADMIN_TOKEN = '';
    expect((await post('')).status).toBe(503);
    expect((await post('anything')).status).toBe(503);
    delete process.env.FIAT_ADMIN_TOKEN;
    expect((await post('anything')).status).toBe(503); // unset also disables
  });

  it('a malformed body is a 400 and never mints a credential (auth passes first)', async () => {
    process.env.FIAT_ADMIN_TOKEN = 'admin-secret';
    expect((await post('admin-secret', { userId: '' })).status).toBe(400);
    expect((await post('admin-secret', { userId: 42 })).status).toBe(400);
    expect((await post('admin-secret', {})).status).toBe(400);
  });
});

// ---------------------------------------------------------------------------
// Stripe signature verify: header edge cases the existing test does not reach
// ---------------------------------------------------------------------------
describe('verifyStripeSignature: hostile header shapes', () => {
  const SECRET = 'whsec_test_secret';
  const NOW_S = 1_700_000_000;
  const payload = '{"id":"evt_1"}';
  const mac = (t: number, s = SECRET, p = payload) => createHmac('sha256', s).update(`${t}.${p}`).digest('hex');
  const at = (nowS = NOW_S) => ({ nowMs: nowS * 1000 });

  it('a v1 that is a truncated (wrong-length) hex prefix of the real mac is rejected', () => {
    const good = mac(NOW_S);
    expect(verifyStripeSignature(payload, `t=${NOW_S},v1=${good.slice(0, 63)}`, SECRET, at())).toBe(false);
    expect(verifyStripeSignature(payload, `t=${NOW_S},v1=${good}0`, SECRET, at())).toBe(false); // too long
  });

  it('a non-hex v1 is rejected (no timingSafeEqual length throw, no coercion)', () => {
    expect(verifyStripeSignature(payload, `t=${NOW_S},v1=${'z'.repeat(64)}`, SECRET, at())).toBe(false);
    expect(verifyStripeSignature(payload, `t=${NOW_S},v1=`, SECRET, at())).toBe(false); // empty
  });

  it('an UPPERCASE-hex v1 equal to the mac is rejected (strict lowercase match)', () => {
    expect(verifyStripeSignature(payload, `t=${NOW_S},v1=${mac(NOW_S).toUpperCase()}`, SECRET, at())).toBe(false);
  });

  it('with several t= the FIRST wins: a stale-first header is rejected even if a fresh t follows', () => {
    // First t is stale -> both the tolerance check and the HMAC use it -> reject.
    const stale = NOW_S - 10_000;
    const header = `t=${stale},t=${NOW_S},v1=${mac(NOW_S)}`;
    expect(verifyStripeSignature(payload, header, SECRET, at())).toBe(false);
    // First t fresh + mac computed for that fresh t -> accepted.
    const header2 = `t=${NOW_S},t=${stale},v1=${mac(NOW_S)}`;
    expect(verifyStripeSignature(payload, header2, SECRET, at())).toBe(true);
  });

  it('a future timestamp is accepted within tolerance and rejected beyond it', () => {
    expect(verifyStripeSignature(payload, `t=${NOW_S + 299},v1=${mac(NOW_S + 299)}`, SECRET, at())).toBe(true);
    expect(verifyStripeSignature(payload, `t=${NOW_S + 301},v1=${mac(NOW_S + 301)}`, SECRET, at())).toBe(false);
  });

  it('a header with a t but zero v1 entries is rejected', () => {
    expect(verifyStripeSignature(payload, `t=${NOW_S}`, SECRET, at())).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// extractPurchase: event smuggling (float/string/negative amount, object user,
// currency case, missing fields). Each MUST fail closed to 'ignored'.
// ---------------------------------------------------------------------------
describe('extractPurchase: hostile Stripe event fields', () => {
  const base = (obj: Record<string, unknown>) => ({
    id: 'evt_1',
    type: 'checkout.session.completed',
    data: { object: { metadata: { fc1UserId: 'user-1' }, amount_total: 500, currency: 'usd', payment_intent: 'pi_1', ...obj } },
  });

  it('amount_total as a float, string, negative, or zero is ignored (no BigInt injection, no negative mint)', () => {
    expect(extractPurchase(base({ amount_total: 5.5 }))).toMatchObject({ kind: 'ignored' });
    expect(extractPurchase(base({ amount_total: '500' }))).toMatchObject({ kind: 'ignored' });
    expect(extractPurchase(base({ amount_total: -500 }))).toMatchObject({ kind: 'ignored' });
    expect(extractPurchase(base({ amount_total: 0 }))).toMatchObject({ kind: 'ignored' });
    expect(extractPurchase(base({ amount_total: Number.NaN }))).toMatchObject({ kind: 'ignored' });
  });

  it('fc1UserId as an object, array, or empty string is ignored (no non-string / empty user)', () => {
    expect(extractPurchase(base({ metadata: { fc1UserId: {} } }))).toMatchObject({ kind: 'ignored' });
    expect(extractPurchase(base({ metadata: { fc1UserId: ['user-1'] } }))).toMatchObject({ kind: 'ignored' });
    expect(extractPurchase(base({ metadata: { fc1UserId: '' } }))).toMatchObject({ kind: 'ignored' });
    expect(extractPurchase(base({ metadata: 'user-1' }))).toMatchObject({ kind: 'ignored' }); // metadata not an object
    expect(extractPurchase(base({ metadata: null }))).toMatchObject({ kind: 'ignored' });
  });

  it('currency is matched case-sensitively: "USD" is ignored, only "usd" credits', () => {
    expect(extractPurchase(base({ currency: 'USD' }))).toMatchObject({ kind: 'ignored' });
    expect(extractPurchase(base({ currency: undefined }))).toMatchObject({ kind: 'ignored' });
    expect(extractPurchase(base({}))).toMatchObject({ kind: 'purchase' }); // usd -> ok
  });

  it('a missing id, missing type, or missing data.object is ignored, never a throw', () => {
    expect(extractPurchase({ type: 'checkout.session.completed', data: { object: {} } })).toMatchObject({ kind: 'ignored' });
    expect(extractPurchase({ id: 42, type: 'checkout.session.completed', data: { object: {} } })).toMatchObject({ kind: 'ignored' });
    expect(extractPurchase({ id: 'evt_1', type: 'checkout.session.completed' })).toMatchObject({ kind: 'ignored' });
    expect(extractPurchase(null)).toMatchObject({ kind: 'ignored' });
    expect(extractPurchase('a string event')).toMatchObject({ kind: 'ignored' });
  });

  it('a non-string payment_intent yields a purchase with NO refundable paymentIntentId (cannot be cashed to a card)', () => {
    const out = extractPurchase(base({ payment_intent: { id: 'pi_evil' } }));
    expect(out).toMatchObject({ kind: 'purchase', userId: 'user-1', amountMicro: 5_000_000n });
    expect((out as { paymentIntentId?: string }).paymentIntentId).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// Webhook: an event id replayed with a DIFFERENT (bigger) amount must NOT
// inflate the balance — idempotency is per-event-id and the FIRST amount wins.
// A hostile userId ("__proto__") must be an isolated user, never a pollution.
// ---------------------------------------------------------------------------
describe('webhook: duplicate event id cannot inflate, hostile userId is isolated', () => {
  const SECRET = 'whsec_test_secret';
  let ledger: CreditsLedger;
  beforeEach(async () => {
    process.env.FIAT_STRIPE_WEBHOOK_SECRET = SECRET;
    ledger = await CreditsLedger.open(new MemoryLedgerStore());
    setFiatDepsForTest({ ledger });
  });
  afterEach(() => {
    delete process.env.FIAT_STRIPE_WEBHOOK_SECRET;
    setFiatDepsForTest(undefined);
  });

  const signNow = (payload: string) =>
    `t=${Math.floor(Date.now() / 1000)},v1=${createHmac('sha256', SECRET).update(`${Math.floor(Date.now() / 1000)}.${payload}`).digest('hex')}`;

  function post(payload: string) {
    return webhookRoute(
      new Request('http://site/api/fiat/stripe/webhook', {
        method: 'POST',
        headers: { 'stripe-signature': signNow(payload) },
        body: payload,
      })
    );
  }
  const checkout = (id: string, cents: number, userId = 'user-1') =>
    JSON.stringify({
      id,
      type: 'checkout.session.completed',
      data: { object: { metadata: { fc1UserId: userId }, amount_total: cents, currency: 'usd', payment_intent: 'pi_1' } },
    });

  it('replaying event id evt_X with a 100x amount is a no-op (first amount stands)', async () => {
    const first = await post(checkout('evt_X', 500)); // $5
    expect((await first.json())).toEqual({ received: true, applied: true });
    expect(ledger.availableMicro('user-1')).toBe(5_000_000n);

    const bumped = await post(checkout('evt_X', 50_000)); // same id, $500 — the attack
    expect(bumped.status).toBe(200);
    expect((await bumped.json())).toEqual({ received: true, applied: false });
    expect(ledger.availableMicro('user-1')).toBe(5_000_000n); // NOT inflated
  });

  it('a userId of "__proto__" credits an isolated user and does not pollute Object.prototype', async () => {
    const res = await post(checkout('evt_proto', 500, '__proto__'));
    expect((await res.json())).toEqual({ received: true, applied: true });
    expect(ledger.availableMicro('__proto__')).toBe(5_000_000n);
    expect(({} as Record<string, unknown>).amount_total).toBeUndefined();
    expect(ledger.availableMicro('polluted')).toBe(0n);
    // A different, honest user is unaffected by the weird key.
    expect(ledger.availableMicro('user-1')).toBe(0n);
  });
});

// ---------------------------------------------------------------------------
// F1 — FIXED (regression guard): concurrent cash-outs must not together refund
// more than a card was charged. The per-charge remaining check now runs INSIDE
// the ledger's serial queue (ledger.cashout, guarded by remainingForCharge),
// so two racing full-charge cash-outs can only settle ONE. Previously the cap
// was checked outside the queue and both raced through — an on-ramp over-refund
// vector. These tests assert the secure behaviour.
// ---------------------------------------------------------------------------
describe('F1 [FIXED]: concurrent cash-outs cannot exceed the original card charge', () => {
  async function seed() {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    // $5 charged to card pi_1, PLUS $5 of render-refund credits (no card) -> $10 balance.
    await ledger.purchase('u', 5_000_000n, 'evt_card', { paymentIntentId: 'pi_1' });
    await ledger.purchase('u', 5_000_000n, 'evt_refund');
    const credentials = await FiatCredentials.open(new MemoryLedgerStore());
    const token = await credentials.issue('u');
    const refunds: Array<{ pi: string; cents: number }> = [];
    const deps: CashoutDeps = {
      ledger,
      credentials,
      stripe: {
        createRefund: async (pi, cents) => {
          refunds.push({ pi, cents });
          return { id: `re_${refunds.length}` };
        },
      },
      refundWindowDays: 90,
    };
    return { ledger, token, deps, refunds };
  }

  it('two racing $5 cash-outs against a $5 charge settle only ONE — never more than the card was charged', async () => {
    const { ledger, token, deps, refunds } = await seed();
    const [a, b] = await Promise.all([
      requestCashout(deps, { credential: token, amountMicro: 5_000_000n }),
      requestCashout(deps, { credential: token, amountMicro: 5_000_000n }),
    ]);

    // Exactly one wins; the other is refused for exceeding the charge remaining.
    const outcomes = [a.status, b.status].sort();
    expect(outcomes).toEqual(['ok', 'refused']);
    const refused = [a, b].find((o) => o.status === 'refused');
    expect(refused).toMatchObject({ status: 'refused', reason: 'EXCEEDS_REFUNDABLE' });

    // Only 500 cents refunded against the 500-cent charge — the invariant holds
    // at the LEDGER level, not just at Stripe.
    const centsToCard = refunds.filter((r) => r.pi === 'pi_1').reduce((s, r) => s + r.cents, 0);
    expect(refunds).toHaveLength(1);
    expect(centsToCard).toBe(500);
    // The user keeps the $5 of non-card render credits (those are not card-refundable).
    expect(ledger.availableMicro('u')).toBe(5_000_000n);
  });

  it('CONTRAST: with no excess non-card balance, the serial BALANCE check DOES stop the second cash-out', async () => {
    // Only the $5 card purchase, no render-refund credits. Balance == charge.
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    await ledger.purchase('u', 5_000_000n, 'evt_card', { paymentIntentId: 'pi_1' });
    const credentials = await FiatCredentials.open(new MemoryLedgerStore());
    const token = await credentials.issue('u');
    const refunds: Array<{ pi: string; cents: number }> = [];
    const deps: CashoutDeps = {
      ledger,
      credentials,
      stripe: { createRefund: async (pi, cents) => { refunds.push({ pi, cents }); return { id: `re_${refunds.length}` }; } },
      refundWindowDays: 90,
    };
    const [a, b] = await Promise.all([
      requestCashout(deps, { credential: token, amountMicro: 5_000_000n }),
      requestCashout(deps, { credential: token, amountMicro: 5_000_000n }),
    ]);
    const outcomes = [a.status, b.status].sort();
    expect(outcomes).toEqual(['ok', 'refused']); // exactly one wins
    expect(refunds).toHaveLength(1); // only one card refund issued
    expect(ledger.availableMicro('u')).toBe(0n);
  });
});

// ---------------------------------------------------------------------------
// Cash-out: R3 window boundary, and reversal is exactly compensating
// ---------------------------------------------------------------------------
describe('cash-out: refund-window boundary is exclusive on ">"', () => {
  const DAY_MS = 86_400_000;
  async function depsAt(ageDays: number) {
    const nowMs = 1_700_000_000_000;
    const purchaseAtMs = nowMs - Math.round(ageDays * DAY_MS);
    let clock = purchaseAtMs;
    const ledger = await CreditsLedger.open(new MemoryLedgerStore(), { now: () => clock });
    const credentials = await FiatCredentials.open(new MemoryLedgerStore());
    const token = await credentials.issue('u');
    await ledger.purchase('u', 5_000_000n, 'evt_1', { paymentIntentId: 'pi_1' });
    clock = nowMs;
    const refunds: Array<{ pi: string; cents: number }> = [];
    const deps: CashoutDeps = {
      ledger,
      credentials,
      stripe: { createRefund: async (pi, cents) => { refunds.push({ pi, cents }); return { id: 're_1' }; } },
      refundWindowDays: 90,
      now: () => nowMs,
    };
    return { deps, token };
  }

  it('exactly 90 days old is still refundable; a millisecond past 90 days is REFUND_WINDOW_EXPIRED', async () => {
    const exact = await depsAt(90);
    expect((await requestCashout(exact.deps, { credential: exact.token, amountMicro: 1_000_000n })).status).toBe('ok');

    // now - atMs must exceed 90*DAY by 1ms to trip ">".
    const nowMs = 1_700_000_000_000;
    let clock = nowMs - 90 * DAY_MS - 1;
    const ledger = await CreditsLedger.open(new MemoryLedgerStore(), { now: () => clock });
    const credentials = await FiatCredentials.open(new MemoryLedgerStore());
    const token = await credentials.issue('u');
    await ledger.purchase('u', 5_000_000n, 'evt_1', { paymentIntentId: 'pi_1' });
    clock = nowMs;
    const deps: CashoutDeps = {
      ledger,
      credentials,
      stripe: { createRefund: async () => ({ id: 're_1' }) },
      refundWindowDays: 90,
      now: () => nowMs,
    };
    expect(await requestCashout(deps, { credential: token, amountMicro: 1_000_000n })).toEqual({
      status: 'refused',
      reason: 'REFUND_WINDOW_EXPIRED',
    });
  });
});

// ---------------------------------------------------------------------------
// Settlement listener: subtle correctness the existing suite does not pin down
// ---------------------------------------------------------------------------
describe('applySettlementEvents: zero-refund guard, same-batch conflicts, depositor field', () => {
  const DEPOSIT = 500_000n;
  const REFUND = 299_217n;

  async function boundJob(jobId: bigint) {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
    if (!open.ok) throw new Error('open refused');
    await ledger.bindSession(open.holdId, jobId);
    return ledger;
  }
  const completed = (jobId: bigint, userRefund: bigint, blockNumber = 10): SettlementEvent => ({
    kind: 'completed',
    jobId,
    userRefund,
    blockNumber,
  });

  it('a zero userRefund (host took everything) settles once and blocks a later resettle — 0n is not treated as "unsettled"', async () => {
    const ledger = await boundJob(900n);
    const alarms: string[] = [];
    const r1 = await applySettlementEvents(ledger, [completed(900n, 0n)], (m) => alarms.push(m));
    expect(r1.settled).toBe(1);
    expect(ledger.refundForJob(900n)).toBe(0n); // recorded as 0n, NOT undefined
    expect(ledger.availableMicro('user-1')).toBe(500_000n); // 1,000,000 - 500,000 spent, 0 back

    // A duplicate 0-refund event is a clean no-op (0n !== undefined guard holds).
    const r2 = await applySettlementEvents(ledger, [completed(900n, 0n)], (m) => alarms.push(m));
    expect(r2.settled).toBe(0);
    expect(alarms).toEqual([]);

    // A later NON-zero refund for the same job is a divergence alarm, never a credit.
    await applySettlementEvents(ledger, [completed(900n, 5n)], (m) => alarms.push(m));
    expect(alarms).toHaveLength(1);
    expect(ledger.availableMicro('user-1')).toBe(500_000n);
  });

  it('two conflicting events for the same job in ONE batch: first settles, second alarms, single credit only', async () => {
    const ledger = await boundJob(901n);
    const alarms: string[] = [];
    const result = await applySettlementEvents(
      ledger,
      [completed(901n, REFUND, 10), completed(901n, REFUND + 1n, 10)],
      (m) => alarms.push(m)
    );
    expect(result.settled).toBe(1);
    expect(alarms).toHaveLength(1);
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n - (DEPOSIT - REFUND)); // one credit
  });

  it('a RefundCreditedToDeposit with a NON-vault depositor still settles by jobId (depositor field is not the authority — the ledger binding is)', async () => {
    const ledger = await boundJob(902n);
    const alarms: string[] = [];
    const result = await applySettlementEvents(
      ledger,
      [{ kind: 'refund-credited', jobId: 902n, amount: REFUND, depositor: OFFLIST, blockNumber: 12 }],
      (m) => alarms.push(m)
    );
    expect(result.settled).toBe(1);
    expect(alarms).toEqual([]);
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n - (DEPOSIT - REFUND));
  });

  it('a refund-credited amount ABOVE the recorded deposit alarms and credits nothing (poison, bounded by deposit)', async () => {
    const ledger = await boundJob(903n);
    const alarms: string[] = [];
    const result = await applySettlementEvents(
      ledger,
      [{ kind: 'refund-credited', jobId: 903n, amount: DEPOSIT + 1n, depositor: VAULT, blockNumber: 12 }],
      (m) => alarms.push(m)
    );
    expect(result.settled).toBe(0);
    expect(alarms).toHaveLength(1);
    expect(alarms[0]).toMatch(/exceeds/i);
    expect(ledger.availableMicro('user-1')).toBe(500_000n); // unchanged (still held-spent)
  });
});
