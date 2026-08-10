// FC1.2 — POST /api/fiat/session: thin HTTP shell over openFiatSession.
// Validation rejects before the service is touched; outcomes map to stable
// status codes; bigints cross the wire as decimal strings.
import { afterEach, describe, expect, it } from 'vitest';
import { POST } from '../app/v1/fiat/session/route';
import {
  setFiatSessionServiceForTest,
  type FiatSessionOutcome,
  type FiatSessionRequest,
} from '../src/lib/fiat-session-service';

const HOST = '0xabcd000000000000000000000000000000000001';
const MODEL = `0x${'ab'.repeat(32)}`;
const CLIENT = '0x1234567890abcdef1234567890abcdef12345678';

function stubService(outcome: FiatSessionOutcome) {
  const seen: FiatSessionRequest[] = [];
  setFiatSessionServiceForTest({
    open: async (req) => {
      seen.push(req);
      return outcome;
    },
  });
  return seen;
}

afterEach(() => setFiatSessionServiceForTest(undefined));

const okOutcome: FiatSessionOutcome = {
  status: 'ok',
  sessionId: 842n,
  jobId: 842n,
  authorisation: { scheme: 'fc1-session-auth-v1', signature: '0xsig', clientAddress: CLIENT },
};

function post(body: unknown, credential?: string) {
  return POST(
    new Request('http://site/api/fiat/session', {
      method: 'POST',
      headers: credential ? { authorization: `Bearer ${credential}` } : {},
      body: typeof body === 'string' ? body : JSON.stringify(body),
    })
  );
}

const GOOD_BODY = { host: HOST, modelId: MODEL, depositMicro: '500000', clientAddress: CLIENT };

describe('POST /api/fiat/session', () => {
  it('opens a session and returns bigints as strings', async () => {
    const seen = stubService(okOutcome);
    const res = await post(GOOD_BODY, 'fc1_token');
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({
      sessionId: '842',
      jobId: '842',
      authorisation: { scheme: 'fc1-session-auth-v1', signature: '0xsig', clientAddress: CLIENT },
    });
    expect(seen).toEqual([
      { credential: 'fc1_token', host: HOST, modelId: MODEL, depositMicro: 500_000n, clientAddress: CLIENT },
    ]);
  });

  it('401s without a bearer credential, before the service is touched', async () => {
    const seen = stubService(okOutcome);
    const res = await post(GOOD_BODY);
    expect(res.status).toBe(401);
    expect(seen).toHaveLength(0);
  });

  it('400s on malformed bodies without touching the service', async () => {
    const seen = stubService(okOutcome);
    expect((await post('not json', 'fc1_t')).status).toBe(400);
    expect((await post({ ...GOOD_BODY, host: 'not-an-address' }, 'fc1_t')).status).toBe(400);
    expect((await post({ ...GOOD_BODY, clientAddress: 'nope' }, 'fc1_t')).status).toBe(400);
    expect((await post({ ...GOOD_BODY, modelId: '0x123' }, 'fc1_t')).status).toBe(400);
    expect((await post({ ...GOOD_BODY, depositMicro: '1.5' }, 'fc1_t')).status).toBe(400);
    expect((await post({ ...GOOD_BODY, depositMicro: '-5' }, 'fc1_t')).status).toBe(400);
    expect((await post({ ...GOOD_BODY, depositMicro: 500000 }, 'fc1_t')).status).toBe(400); // must be a string
    expect(seen).toHaveLength(0);
  });

  it('maps unauthorised to 401', async () => {
    stubService({ status: 'unauthorised' });
    expect((await post(GOOD_BODY, 'fc1_bad')).status).toBe(401);
  });

  it('maps a gatekeeper refusal to 403 with the machine-readable reason', async () => {
    stubService({ status: 'refused', reason: 'INSUFFICIENT_BALANCE' });
    const res = await post(GOOD_BODY, 'fc1_t');
    expect(res.status).toBe(403);
    expect(await res.json()).toEqual({ error: 'refused', reason: 'INSUFFICIENT_BALANCE' });
  });

  it('maps chain errors to 502 without leaking internals beyond the message', async () => {
    stubService({ status: 'chain_error', message: 'tx reverted' });
    const res = await post(GOOD_BODY, 'fc1_t');
    expect(res.status).toBe(502);
    expect(await res.json()).toEqual({ error: 'chain_error', message: 'tx reverted' });
  });

  it('503s when the fiat backend is not configured (service factory throws)', async () => {
    setFiatSessionServiceForTest(undefined); // fall through to the env-built service
    delete process.env.FIAT_VAULT_PRIVATE_KEY;
    const res = await post(GOOD_BODY, 'fc1_t');
    expect(res.status).toBe(503);
    expect((await res.json()).error).toContain('not configured');
  });
});
