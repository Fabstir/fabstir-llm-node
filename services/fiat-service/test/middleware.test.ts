// Locks the CORS contract of middleware.ts (EXECUTION goal 2), which the ported
// route tests cannot see (they invoke handlers directly, bypassing middleware).
// The behaviors asserted here were first verified live (WORKLOG round 1):
//  - browser routes: preflight answered, ACAO for allowed origins only, no
//    credentials, Cache-Control: no-store stamped on actual responses;
//  - the operator route (/v1/fiat/credential, EXACT) and the Stripe webhook get
//    NO browser CORS, while /credential's children DO.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { NextRequest } from 'next/server';
import { middleware } from '../middleware';

const UI = 'http://ui.example';

function req(path: string, opts: { method?: string; origin?: string } = {}): NextRequest {
  const headers = new Headers();
  if (opts.origin) headers.set('origin', opts.origin);
  return new NextRequest(`http://svc.local${path}`, { method: opts.method ?? 'GET', headers });
}

// NextResponse.next() marks pass-through with this internal header.
function isPassThrough(res: Response): boolean {
  return res.headers.get('x-middleware-next') === '1';
}

const savedEnv = process.env.FIAT_CORS_ALLOWED_ORIGINS;
beforeEach(() => {
  process.env.FIAT_CORS_ALLOWED_ORIGINS = UI;
});
afterEach(() => {
  if (savedEnv === undefined) delete process.env.FIAT_CORS_ALLOWED_ORIGINS;
  else process.env.FIAT_CORS_ALLOWED_ORIGINS = savedEnv;
});

describe('preflight (OPTIONS) on browser routes', () => {
  it('answers 204 with the full CORS header set for an allowed origin', () => {
    const res = middleware(req('/v1/fiat/balance', { method: 'OPTIONS', origin: UI }));
    expect(res.status).toBe(204);
    expect(isPassThrough(res)).toBe(false); // answered here, never reaches the handler
    expect(res.headers.get('access-control-allow-origin')).toBe(UI);
    expect(res.headers.get('access-control-allow-methods')).toContain('GET');
    expect(res.headers.get('access-control-allow-headers')).toContain('authorization');
    expect(res.headers.get('access-control-max-age')).toBe('86400');
    expect(res.headers.get('vary')).toContain('Origin');
    // no cookies: credentials must never be allowed
    expect(res.headers.get('access-control-allow-credentials')).toBeNull();
  });

  it('answers 204 with NO CORS headers for a disallowed origin', () => {
    const res = middleware(req('/v1/fiat/balance', { method: 'OPTIONS', origin: 'http://evil.example' }));
    expect(res.status).toBe(204);
    expect(res.headers.get('access-control-allow-origin')).toBeNull();
  });

  it('normalises one trailing slash', () => {
    const res = middleware(req('/v1/fiat/balance/', { method: 'OPTIONS', origin: UI }));
    expect(res.status).toBe(204);
    expect(res.headers.get('access-control-allow-origin')).toBe(UI);
  });

  it("'*' config answers with a literal * and no Vary", () => {
    process.env.FIAT_CORS_ALLOWED_ORIGINS = '*';
    const res = middleware(req('/v1/fiat/purchase', { method: 'OPTIONS', origin: 'http://anyone.example' }));
    expect(res.headers.get('access-control-allow-origin')).toBe('*');
    expect(res.headers.get('vary')).toBeNull();
  });

  it('unset config allows no origin at all', () => {
    delete process.env.FIAT_CORS_ALLOWED_ORIGINS;
    const res = middleware(req('/v1/fiat/balance', { method: 'OPTIONS', origin: UI }));
    expect(res.headers.get('access-control-allow-origin')).toBeNull();
  });
});

describe('actual requests on browser routes', () => {
  it('passes through with ACAO + Cache-Control: no-store for an allowed origin', () => {
    const res = middleware(req('/v1/fiat/balance', { origin: UI }));
    expect(isPassThrough(res)).toBe(true);
    expect(res.headers.get('access-control-allow-origin')).toBe(UI);
    expect(res.headers.get('cache-control')).toBe('no-store');
  });

  it('still stamps no-store when there is no Origin (same-origin/curl)', () => {
    const res = middleware(req('/v1/fiat/balance'));
    expect(isPassThrough(res)).toBe(true);
    expect(res.headers.get('access-control-allow-origin')).toBeNull();
    expect(res.headers.get('cache-control')).toBe('no-store');
  });

  it('every browser route is covered', () => {
    for (const p of [
      '/v1/fiat/purchase',
      '/v1/fiat/balance',
      '/v1/fiat/session',
      '/v1/fiat/cashout',
      '/v1/fiat/credential/challenge',
      '/v1/fiat/credential/self-serve',
    ]) {
      const res = middleware(req(p, { method: 'POST', origin: UI }));
      expect(res.headers.get('access-control-allow-origin'), p).toBe(UI);
      expect(res.headers.get('cache-control'), p).toBe('no-store');
    }
  });
});

describe('non-browser routes are untouched (server-to-server)', () => {
  it.each([['/v1/fiat/stripe/webhook'], ['/v1/fiat/credential']])('%s gets no CORS and no cache stamp', (path) => {
    const pre = middleware(req(path, { method: 'OPTIONS', origin: UI }));
    expect(isPassThrough(pre)).toBe(true); // preflight NOT answered by us
    expect(pre.headers.get('access-control-allow-origin')).toBeNull();

    const post = middleware(req(path, { method: 'POST', origin: UI }));
    expect(isPassThrough(post)).toBe(true);
    expect(post.headers.get('access-control-allow-origin')).toBeNull();
    expect(post.headers.get('cache-control')).toBeNull();
  });

  it('the operator route is excluded EXACTLY while its children get CORS', () => {
    const parent = middleware(req('/v1/fiat/credential', { method: 'OPTIONS', origin: UI }));
    expect(parent.headers.get('access-control-allow-origin')).toBeNull();
    const child = middleware(req('/v1/fiat/credential/challenge', { method: 'OPTIONS', origin: UI }));
    expect(child.headers.get('access-control-allow-origin')).toBe(UI);
  });
});
