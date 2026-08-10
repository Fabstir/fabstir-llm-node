import { NextResponse, type NextRequest } from 'next/server';

// CORS for the fiat service, scoped to the BROWSER-facing routes only.
//
// Goal (EXECUTION-FIAT-SERVICE-EXTRACTION.md #2): the buy / balance / session /
// cashout / credential-challenge / credential-self-serve routes are called from
// a browser on another origin (the Platformless AI UI, the Blender website) and
// need CORS. They are self-authenticating (a backend-key signature or a spending
// credential in the body) and use NO cookies, so we allow the configured origins
// with credentials OFF. The Stripe webhook and the operator credential route are
// server-to-server; they get NO browser CORS and are matched out below.
//
// Allowed origins come from FIAT_CORS_ALLOWED_ORIGINS (comma-separated). A single
// "*" allows any origin (fine here: no cookies, self-authenticating). Prefer
// explicit origins in production.

// Exact browser-facing paths. Exact match keeps the operator route
// (/v1/fiat/credential) OUT while letting its browser children in.
const BROWSER_ROUTES = new Set<string>([
  '/v1/fiat/purchase',
  '/v1/fiat/balance',
  '/v1/fiat/session',
  '/v1/fiat/cashout',
  '/v1/fiat/credential/challenge',
  '/v1/fiat/credential/self-serve',
]);

function isBrowserRoute(pathname: string): boolean {
  // Normalise a single trailing slash so "/v1/fiat/balance/" also matches.
  const p = pathname.length > 1 && pathname.endsWith('/') ? pathname.slice(0, -1) : pathname;
  return BROWSER_ROUTES.has(p);
}

function allowedOrigins(): string[] {
  return (process.env.FIAT_CORS_ALLOWED_ORIGINS ?? '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);
}

// Resolve the Access-Control-Allow-Origin value for this request, or null if the
// origin is not allowed. With "*" configured we echo nothing but "*". Otherwise
// we echo the request's origin only when it is in the allowlist (so we can add
// Vary: Origin and keep a single-origin response correct behind a cache).
function resolveAllowOrigin(origin: string | null): string | null {
  const allow = allowedOrigins();
  if (allow.includes('*')) return '*';
  if (origin && allow.includes(origin)) return origin;
  return null;
}

function corsHeaders(allowOrigin: string): Headers {
  const h = new Headers();
  h.set('Access-Control-Allow-Origin', allowOrigin);
  h.set('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
  h.set('Access-Control-Allow-Headers', 'content-type, authorization');
  h.set('Access-Control-Max-Age', '86400');
  // NO Access-Control-Allow-Credentials: these routes use no cookies.
  if (allowOrigin !== '*') h.append('Vary', 'Origin');
  return h;
}

export function middleware(req: NextRequest): NextResponse {
  const { pathname } = req.nextUrl;
  if (!isBrowserRoute(pathname)) return NextResponse.next();

  const allowOrigin = resolveAllowOrigin(req.headers.get('origin'));

  // Preflight: answer here, never reach the handler.
  if (req.method === 'OPTIONS') {
    // Disallowed origin → 204 with no CORS headers; the browser blocks it.
    if (!allowOrigin) return new NextResponse(null, { status: 204 });
    return new NextResponse(null, { status: 204, headers: corsHeaders(allowOrigin) });
  }

  // Actual request: run the handler, add CORS headers when the origin is allowed.
  //
  // Two cache-related notes, established by live probing (WORKLOG round 1):
  // - Next 14 REPLACES the response's Vary header with its own router values
  //   (`RSC, Next-Router-State-Tree, …`), so appending `Vary: Origin` here does
  //   not survive to the client. Rather than fight the framework, we make Vary
  //   irrelevant by ensuring these responses are never cacheable (below).
  // - Route handlers that don't set cache-control (e.g. GET /balance) would be
  //   heuristically cacheable (200 + GET + no cache-control), which is wrong for
  //   a money display AND would combine badly with the missing Vary. So every
  //   browser-facing fiat response is stamped no-store. The two routes that set
  //   it themselves (challenge, self-serve) set the identical value, so merge
  //   order cannot matter.
  const res = NextResponse.next();
  res.headers.set('Cache-Control', 'no-store');
  if (allowOrigin) {
    res.headers.set('Access-Control-Allow-Origin', allowOrigin);
  }
  return res;
}

export const config = {
  matcher: '/v1/fiat/:path*',
};
