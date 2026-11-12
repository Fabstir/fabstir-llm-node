// D20 item 5 (BM1.2c) — the two new authenticated read routes the helper's
// card path uses: GET /v1/fiat/session-key/{key} ("what happened to this
// payment", per user only) and GET /v1/fiat/headroom (what this payer can
// still open, and the stable opaque payerId), plus the session route's two
// new answers (chain_pending, vault_stuck).
import { afterEach, describe, expect, it } from 'vitest';
import { GET as sessionKey } from '../app/v1/fiat/session-key/[key]/route';
import { GET as headroom } from '../app/v1/fiat/headroom/route';
import { POST as session } from '../app/v1/fiat/session/route';
import { setFiatDepsForTest, setFiatSessionServiceForTest } from '../src/lib/fiat-session-service';
import { fiatPayerId } from '../src/lib/fiat-identity';
import { requestFingerprint } from '../src/lib/idempotency';
import { HOST, MODEL } from './helpers/scripted-chain';
import { CLIENT, CONFIG, USER, makeHarness } from './helpers/vault-harness';

const OTHER = '0x2222222222222222222222222222222222222222';
const fp = requestFingerprint({ host: HOST, modelId: MODEL, depositMicro: 500_000n, clientAddress: CLIENT });

afterEach(() => {
  setFiatDepsForTest(undefined);
  setFiatSessionServiceForTest(undefined);
});

function get(url: string, credential?: string) {
  return new Request(url, { headers: credential ? { authorization: `Bearer ${credential}` } : {} });
}

describe('GET /v1/fiat/session-key/{key}', () => {
  it('answers none | pending | done(jobId) with the hold state, for the credential\'s own user ONLY', async () => {
    // mutation: look the key up under a user from the request (or across
    // users) → another user's key state leaks.
    const h = await makeHarness();
    setFiatDepsForTest(h.deps);
    const otherToken = await h.credentials.issue(OTHER);
    // pending, with a held hold
    await h.idempotency.reserve(USER, 'k-pending', fp, CLIENT);
    await h.ledger.openHold({ userId: USER, host: HOST, depositMicro: 500_000n, modelId: MODEL, key: 'k-pending' }, h.deps.gatekeeper);
    // done, bound
    await h.idempotency.reserve(USER, 'k-done', fp, CLIENT);
    const open = await h.ledger.openHold({ userId: USER, host: HOST, depositMicro: 500_000n, modelId: MODEL, key: 'k-done' }, h.deps.gatekeeper);
    if (!open.ok) throw new Error('refused');
    await h.ledger.bindSession(open.holdId, 991n);
    await h.idempotency.complete(USER, 'k-done', 991n, CLIENT);

    const ask = async (key: string, token: string) => {
      const res = await sessionKey(get(`http://svc/v1/fiat/session-key/${key}`, token), { params: { key } });
      return { status: res.status, body: await res.json() };
    };
    expect(await ask('k-none', h.token)).toEqual({ status: 200, body: { state: 'none' } });
    expect(await ask('k-pending', h.token)).toEqual({ status: 200, body: { state: 'pending', holdState: 'held' } });
    expect(await ask('k-done', h.token)).toEqual({ status: 200, body: { state: 'done', jobId: '991', holdState: 'bound' } });
    // Another user asking about the same key names sees nothing.
    expect(await ask('k-pending', otherToken)).toEqual({ status: 200, body: { state: 'none' } });
    expect(await ask('k-done', otherToken)).toEqual({ status: 200, body: { state: 'none' } });
    // No credential, or a bad one.
    expect((await sessionKey(get('http://svc/v1/fiat/session-key/k-done'), { params: { key: 'k-done' } })).status).toBe(401);
    expect((await ask('k-done', 'fc1_nope')).status).toBe(401);
  });

  it('a key containing a literal % round-trips (Next 14.2 has already decoded the segment)', async () => {
    // mutation: decode params.key again → '50%off' throws (a 400) and 'a%41'
    // becomes 'aA' (another key's state, here none).
    const h = await makeHarness();
    setFiatDepsForTest(h.deps);
    for (const [key, jobId] of [['50%off', 992n], ['a%41', 993n], ['%E0%A4%A', 994n]] as const) {
      await h.idempotency.reserve(USER, key, fp, CLIENT);
      await h.idempotency.complete(USER, key, jobId, CLIENT);
      // The params Next hands the route for `/session-key/${encodeURIComponent(key)}`.
      const res = await sessionKey(get(`http://svc/v1/fiat/session-key/${encodeURIComponent(key)}`, h.token), { params: { key } });
      expect(res.status).toBe(200);
      expect(await res.json()).toEqual({ state: 'done', jobId: jobId.toString() });
    }
  });

  it('an empty or over-200-character key answers 400', async () => {
    const h = await makeHarness();
    setFiatDepsForTest(h.deps);
    for (const key of ['', 'k'.repeat(201)]) {
      const res = await sessionKey(get('http://svc/v1/fiat/session-key/x', h.token), { params: { key } });
      expect(res.status).toBe(400);
      expect(await res.json()).toEqual({ error: 'key must be 1..200 characters' });
    }
    const ok = await sessionKey(get('http://svc/v1/fiat/session-key/x', h.token), { params: { key: 'k'.repeat(200) } });
    expect(ok.status).toBe(200);
  });
});

describe('GET /v1/fiat/headroom', () => {
  it('returns payerId, availableMicro, spentInWindowMicro, the daily cap, the per-session cap and opens per minute', async () => {
    const h = await makeHarness();
    setFiatDepsForTest(h.deps);
    await h.ledger.openHold({ userId: USER, host: HOST, depositMicro: 700_000n, modelId: MODEL }, h.deps.gatekeeper);
    const res = await headroom(get('http://svc/v1/fiat/headroom', h.token));
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({
      payerId: fiatPayerId(USER),
      kind: 'standard',
      availableMicro: '9300000',
      spentInWindowMicro: '700000',
      dailyCapMicro: CONFIG.maxDailySpendMicro.toString(),
      sessionCapMicro: CONFIG.maxDepositPerSessionMicro.toString(),
      opensPerMinute: CONFIG.maxOpensPerMinute,
    });
    expect((await headroom(get('http://svc/v1/fiat/headroom'))).status).toBe(401);
  });

  it('?kind=passes answers the passes per-session cap with the STANDARD daily window figures (D21)', async () => {
    // mutation: refuse passes → 400.
    const h = await makeHarness();
    const config = {
      ...CONFIG,
      perKind: { ...CONFIG.perKind, passes: { maxDepositPerSessionMicro: 4_000_000n, maxDailySpendMicro: CONFIG.maxDailySpendMicro } },
      passesModelIds: [`0x${'d2'.repeat(32)}`],
    };
    setFiatDepsForTest({ ...h.deps, config });
    // A STANDARD hold: passes spend shares its window.
    await h.ledger.openHold({ userId: USER, host: HOST, depositMicro: 700_000n, modelId: MODEL }, h.deps.gatekeeper);
    const res = await headroom(get('http://svc/v1/fiat/headroom?kind=passes', h.token));
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({
      payerId: fiatPayerId(USER),
      kind: 'passes',
      availableMicro: '9300000',
      spentInWindowMicro: '700000',
      dailyCapMicro: CONFIG.maxDailySpendMicro.toString(),
      sessionCapMicro: '4000000',
      opensPerMinute: CONFIG.maxOpensPerMinute,
    });
    const bad = await headroom(get('http://svc/v1/fiat/headroom?kind=video', h.token));
    expect(bad.status).toBe(400);
    expect(await bad.json()).toEqual({ error: "kind must be 'standard', 'training' or 'passes' when present" });
  });

  it('the payerId is stable for one user across credentials, opaque, and different between users', async () => {
    // mutation: derive it from the credential → a re-pair under the same
    // account reads as a different payer.
    const h = await makeHarness();
    setFiatDepsForTest(h.deps);
    const payer = async (token: string) => ((await (await headroom(get('http://svc/v1/fiat/headroom', token))).json()) as { payerId: string }).payerId;
    const first = await payer(h.token);
    const second = await payer(await h.credentials.issue(USER, 'browser'));
    const other = await payer(await h.credentials.issue(OTHER));
    expect(second).toBe(first);
    expect(other).not.toBe(first);
    expect(first).not.toContain(USER.slice(2));
  });
});

describe('POST /v1/fiat/session: the D20 answers', () => {
  const body = { host: HOST, modelId: MODEL, depositMicro: '500000', clientAddress: CLIENT };
  const post = () =>
    session(new Request('http://svc/v1/fiat/session', { method: 'POST', headers: { authorization: 'Bearer t' }, body: JSON.stringify(body) }));

  it('chain_pending → 502 {error: chain_pending}', async () => {
    setFiatSessionServiceForTest({ open: async () => ({ status: 'chain_pending', message: 'unconfirmed' }) });
    const res = await post();
    expect(res.status).toBe(502);
    expect(await res.json()).toMatchObject({ error: 'chain_pending' });
  });

  it('vault_stuck → 503 {error: vault_stuck}', async () => {
    setFiatSessionServiceForTest({ open: async () => ({ status: 'vault_stuck' }) });
    const res = await post();
    expect(res.status).toBe(503);
    expect(await res.json()).toEqual({ error: 'vault_stuck' });
  });
});
