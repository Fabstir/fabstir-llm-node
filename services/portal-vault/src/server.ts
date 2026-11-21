// The HTTP layer: plain node:http, no web framework. Routing, the body limit
// and the rate limit (R7), CORS (R8), JSON errors, and graceful shutdown. All
// decisions about vaults are in handlers.ts; this file only carries requests
// to them and answers back.
import { realpathSync } from 'node:fs';
import http, { type IncomingMessage, type ServerResponse } from 'node:http';
import { isIP } from 'node:net';
import { fileURLToPath } from 'node:url';
import { ChallengeStore } from './challenge.js';
import { ConfigError, MAX_BODY_BYTES, loadConfig, type Config } from './config.js';
import {
  handleChallenge,
  handleCurrent,
  handleHealth,
  handleOpen,
  handleReplace,
  type HandlerDeps,
  type HandlerResult,
  type LogEntry,
} from './handlers.js';
import { RateLimiter, clientKey } from './ratelimit.js';
import { createSignatureVerifier, withConcurrencyLimit, type SignatureVerifier } from './signature.js';
import { S3VaultStore } from './store-s3.js';
import { AccountLocks, type VaultStore } from './store.js';

const PREFIX = '/vault/v1/';
const POST_ROUTES = new Set(['challenge', 'open', 'replace']);
const GET_ROUTES = new Set(['current', 'health']);
// Past the body limit the rest of the request is read and thrown away so the
// refusal can be delivered cleanly; beyond this much the connection is cut.
const MAX_DISCARD_BYTES = 1024 * 1024;
/** R1 v0.2: signature verifications in flight at once, and (R7) per client per minute. */
export const MAX_VERIFICATIONS_IN_FLIGHT = 16;
export const VERIFICATIONS_PER_CLIENT_PER_MINUTE = 10;
/** How long a caller waits for the storage before it is answered 503 (R6a). */
export const STORAGE_TIMEOUT_MS = 10_000;
/** /health is outside the rate limit, so its storage check is remembered for
 *  this long: calling it often cannot be turned into load on the storage. */
const HEALTH_CACHE_MS = 5_000;

export interface VaultServerOptions {
  deps: HandlerDeps;
  rateLimiter: RateLimiter;
  /** R8: exact origins that receive CORS headers. Empty: none. */
  allowedOrigins: readonly string[];
  maxBodyBytes?: number;
}

function isLoopback(address: string): boolean {
  return address === '::1' || /^127\.\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(address);
}

function stripMapped(address: string): string {
  return address.startsWith('::ffff:') && isIP(address.slice(7)) === 4 ? address.slice(7) : address;
}

/** R7: the client address the rate limit counts by. X-Forwarded-For is
 *  believed only when the connection itself comes from the loopback address
 *  (the reverse proxy on the same box). Of several entries the LAST is taken:
 *  it is the one the nearest proxy added, and the only one the caller cannot
 *  choose. Anything that is not an IP address is ignored. */
export function clientAddress(remoteAddress: string | undefined, forwardedFor: string | string[] | undefined): string {
  if (typeof remoteAddress !== 'string' || remoteAddress.length === 0) return 'unknown';
  const remote = stripMapped(remoteAddress);
  if (!isLoopback(remote)) return remote;
  const header = Array.isArray(forwardedFor) ? forwardedFor.join(',') : forwardedFor;
  if (typeof header !== 'string' || header.length === 0) return remote;
  const last = header.split(',').pop()!.trim();
  return isIP(last) !== 0 ? stripMapped(last) : remote;
}

type BodyResult = { ok: true; text: string } | { ok: false; reason: 'too-large' | 'aborted' };

function readBody(req: IncomingMessage, limit: number): Promise<BodyResult> {
  return new Promise((resolve) => {
    const declared = Number(req.headers['content-length']);
    let tooLarge = Number.isFinite(declared) && declared > limit;
    const chunks: Buffer[] = [];
    let size = 0;
    let settled = false;
    const settle = (result: BodyResult) => {
      if (settled) return;
      settled = true;
      resolve(result);
    };
    req.on('data', (chunk: Buffer) => {
      size += chunk.length;
      if (!tooLarge && size > limit) {
        tooLarge = true;
        chunks.length = 0;
      }
      if (tooLarge) {
        if (size > MAX_DISCARD_BYTES) {
          req.destroy();
          settle({ ok: false, reason: 'aborted' });
        }
        return;
      }
      chunks.push(chunk);
    });
    req.on('end', () => settle(tooLarge ? { ok: false, reason: 'too-large' } : { ok: true, text: Buffer.concat(chunks).toString('utf8') }));
    req.on('error', () => settle({ ok: false, reason: 'aborted' }));
    req.on('close', () => settle({ ok: false, reason: 'aborted' }));
  });
}

export function createVaultServer(opts: VaultServerOptions): http.Server {
  const { deps, rateLimiter } = opts;
  const allowedOrigins = new Set(opts.allowedOrigins);
  const maxBodyBytes = opts.maxBodyBytes ?? MAX_BODY_BYTES;

  function corsHeaders(req: IncomingMessage): Record<string, string> {
    // R8: only for an origin on the list, compared exactly. Never a wildcard,
    // never credentials.
    const origin = req.headers.origin;
    if (typeof origin !== 'string' || !allowedOrigins.has(origin)) return {};
    return {
      'Access-Control-Allow-Origin': origin,
      'Access-Control-Allow-Headers': 'Content-Type, Authorization',
      'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
      'Access-Control-Max-Age': '600',
      Vary: 'Origin',
    };
  }

  function send(res: ServerResponse, cors: Record<string, string>, result: HandlerResult): void {
    if (res.headersSent || res.destroyed) return;
    const payload = Buffer.from(JSON.stringify(result.body), 'utf8');
    res.writeHead(result.status, {
      'Content-Type': 'application/json; charset=utf-8',
      'Content-Length': String(payload.length),
      'Cache-Control': 'no-store',
      'X-Content-Type-Options': 'nosniff',
      ...cors,
      ...(result.headers ?? {}),
    });
    res.end(payload);
  }

  /** Refusals made here, before any handler runs, get their own log line. */
  function refuse(
    res: ServerResponse,
    cors: Record<string, string>,
    status: number,
    error: string,
    headers?: Record<string, string>,
    client?: string
  ): void {
    const entry: LogEntry = { call: 'http', outcome: error, status, ms: 0 };
    if (client !== undefined) entry.client = client; // R9 v0.2: a 429 names the client key
    deps.log(entry);
    send(res, cors, headers ? { status, body: { error }, headers } : { status, body: { error } });
  }

  let health: { at: number; result: Promise<HandlerResult> } | undefined;
  function healthAnswer(): Promise<HandlerResult> {
    const t = deps.now();
    if (!health || t - health.at >= HEALTH_CACHE_MS || t < health.at) {
      health = { at: t, result: handleHealth(deps) };
    }
    return health.result;
  }

  async function handle(req: IncomingMessage, res: ServerResponse): Promise<void> {
    const cors = corsHeaders(req);

    const method = req.method ?? '';
    const rawUrl = req.url ?? '';
    const queryAt = rawUrl.indexOf('?');
    const path = queryAt === -1 ? rawUrl : rawUrl.slice(0, queryAt);
    const route = path.startsWith(PREFIX) ? path.slice(PREFIX.length) : '';
    const isPost = POST_ROUTES.has(route);
    const isGet = GET_ROUTES.has(route);

    // R7: the client is an IPv4 address or an IPv6 /64. /health is outside the
    // limiter (v0.2), so a monitor can neither be refused nor use up the
    // allowance of a real caller behind the same address.
    const client = clientKey(clientAddress(req.socket.remoteAddress, req.headers['x-forwarded-for']));
    const isHealth = route === 'health' && method === 'GET';
    if (!isHealth) {
      const rate = rateLimiter.check(client);
      if (!rate.allowed) {
        // The body, if any, is not read; the connection is closed after the answer.
        refuse(res, cors, 429, 'rate-limited', { 'Retry-After': String(rate.retryAfterSeconds), Connection: 'close' }, client);
        return;
      }
    }

    if (method === 'OPTIONS') {
      // R8: a preflight is always answered; the CORS headers are there only
      // for a listed origin, so a browser on any other origin stops here.
      res.writeHead(204, { 'Cache-Control': 'no-store', 'Content-Length': '0', ...cors });
      res.end();
      return;
    }

    if (!isPost && !isGet) {
      refuse(res, cors, 404, 'not-found');
      return;
    }
    if (isPost && method !== 'POST') {
      refuse(res, cors, 405, 'method-not-allowed', { Allow: 'POST, OPTIONS' });
      return;
    }
    if (isGet && method !== 'GET') {
      refuse(res, cors, 405, 'method-not-allowed', { Allow: 'GET, OPTIONS' });
      return;
    }

    if (isGet) {
      if (route === 'health') {
        send(res, cors, await healthAnswer());
        return;
      }
      const query = new URLSearchParams(queryAt === -1 ? '' : rawUrl.slice(queryAt + 1));
      const one = (name: string): string | undefined => {
        const all = query.getAll(name);
        return all.length === 1 ? all[0] : undefined; // absent or repeated: refused by the handler
      };
      const authorization = req.headers.authorization;
      send(res, cors, await handleCurrent(deps, { address: one('address'), chainId: one('chainId'), authorization }));
      return;
    }

    const read = await readBody(req, maxBodyBytes);
    if (!read.ok) {
      if (read.reason === 'too-large') refuse(res, cors, 413, 'body-too-large', { Connection: 'close' });
      return; // aborted: nobody is left to answer
    }
    let body: unknown;
    try {
      body = JSON.parse(read.text);
    } catch {
      refuse(res, cors, 400, 'invalid-request');
      return;
    }
    const result =
      route === 'challenge'
        ? await handleChallenge(deps, body, client)
        : route === 'open'
          ? await handleOpen(deps, body, client)
          : await handleReplace(deps, body, client);
    send(res, cors, result);
  }

  const server = http.createServer((req, res) => {
    handle(req, res).catch(() => {
      // Nothing of the error is logged or returned: it could carry request data.
      deps.log({ call: 'http', outcome: 'internal-error', status: 500, ms: 0 });
      send(res, {}, { status: 500, body: { error: 'internal-error' } });
    });
  });
  server.headersTimeout = 10_000;
  server.requestTimeout = 20_000;
  server.keepAliveTimeout = 5_000;
  server.maxHeadersCount = 64;
  return server;
}

// ---------------------------------------------------------------------------
// Start-up (node dist/server.js)
// ---------------------------------------------------------------------------

function writeLine(stream: NodeJS.WriteStream, value: Record<string, unknown>): void {
  stream.write(JSON.stringify(value) + '\n');
}

/** Parts a test may supply in place of the real ones. Everything else, and in
 *  particular every limit, is built exactly as it runs in production. */
export interface RuntimeOverrides {
  /** In place of the S3 store. */
  store?: VaultStore;
  /** In place of the on-chain verifier. It is still put behind the limit on
   *  checks in flight. */
  verifier?: SignatureVerifier;
  now?: () => number;
  log?: (entry: LogEntry) => void;
  /** Tests shorten these two; production uses the constants above. */
  storageTimeoutMs?: number;
  sweepIntervalMs?: number;
}

export interface Runtime {
  deps: HandlerDeps;
  rateLimiter: RateLimiter;
  server: http.Server;
  /** The sweep timers, all unref'd. */
  timers: NodeJS.Timeout[];
}

/** Builds everything the service runs with from its configuration: the one
 *  place where the protections are put together. `main()` calls this and adds
 *  only what belongs to the process (listening, signals). The wiring tests
 *  call it too and check each protection, so none can be dropped from here
 *  without a test going red. */
export function buildRuntime(config: Config, over: RuntimeOverrides = {}): Runtime {
  const now = over.now ?? (() => Date.now());
  // R9: one JSON line per call: time, call, address, purpose, outcome, status,
  // ms, and where they apply the kind of a storage failure and the client key.
  const log = over.log ?? ((entry: LogEntry) => writeLine(process.stdout, { time: new Date().toISOString(), ...entry }));
  const rawVerifier = over.verifier ?? createSignatureVerifier({ chainId: config.chainId, rpcUrls: config.rpcUrls });

  const deps: HandlerDeps = {
    config: {
      region: config.region,
      portal: config.portalHost,
      chainId: config.chainId,
      tokenSecret: config.tokenSecret,
      readTokenSeconds: config.readTokenDays * 24 * 3600,
      maxVaultBytes: config.maxVaultBytes,
      // R6a: how long a caller waits for the storage before it is answered 503.
      storageTimeoutMs: over.storageTimeoutMs ?? STORAGE_TIMEOUT_MS,
    },
    store: over.store ?? new S3VaultStore(config.s3),
    challenges: new ChallengeStore({ now, max: config.challengeMax }),
    // R1: at most 16 signature checks in flight.
    verify: withConcurrencyLimit(rawVerifier, MAX_VERIFICATIONS_IN_FLIGHT),
    // R7: at most 10 signature checks per client per minute.
    verifyLimiter: new RateLimiter({ perMinute: VERIFICATIONS_PER_CLIENT_PER_MINUTE, now }),
    now,
    log,
    locks: new AccountLocks(),
  };
  const rateLimiter = new RateLimiter({ perMinute: config.ratePerMin, now });
  const server = createVaultServer({ deps, rateLimiter, allowedOrigins: config.allowedOrigins });
  // R7 v0.2: expired entries are released on timers (unref'd: they never keep
  // the process alive), never by a full sweep on a call.
  const timers = [
    rateLimiter.startSweeping(over.sweepIntervalMs),
    deps.verifyLimiter.startSweeping(over.sweepIntervalMs),
    deps.challenges.startSweeping(over.sweepIntervalMs),
  ];
  return { deps, rateLimiter, server, timers };
}

export async function main(env: Record<string, string | undefined> = process.env): Promise<void> {
  let config: Config;
  try {
    config = loadConfig(env);
  } catch (err) {
    if (err instanceof ConfigError) {
      // R10: refuse to start, and name what is missing (names only, no values).
      process.stderr.write(`portal-vault: ${err.message}\n`);
      process.exitCode = 1;
      return;
    }
    throw err;
  }

  const { server } = buildRuntime(config);

  let stopping = false;
  const stop = (signal: string) => {
    if (stopping) return;
    stopping = true;
    writeLine(process.stdout, { time: new Date().toISOString(), event: 'stopping', signal });
    // Stop accepting, let requests in flight finish, then leave.
    server.close(() => process.exit(0));
    server.closeIdleConnections();
    setTimeout(() => process.exit(0), 10_000).unref();
  };
  process.on('SIGTERM', () => stop('SIGTERM'));
  process.on('SIGINT', () => stop('SIGINT'));

  server.on('error', (err) => {
    process.stderr.write(`portal-vault: cannot listen: ${(err as NodeJS.ErrnoException).code ?? 'error'}\n`);
    process.exit(1);
  });
  server.listen(config.port, config.bind, () => {
    writeLine(process.stdout, {
      time: new Date().toISOString(),
      event: 'listening',
      bind: config.bind,
      port: config.port,
      region: config.region,
      portal: config.portalHost,
      chainId: config.chainId,
      rpcUrls: config.rpcUrls.length,
      allowedOrigins: config.allowedOrigins.length,
    });
  });
}

function isEntryPoint(): boolean {
  const entry = process.argv[1];
  if (!entry) return false;
  try {
    return realpathSync(entry) === realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
}

if (isEntryPoint()) {
  main().catch(() => {
    process.stderr.write('portal-vault: failed to start\n');
    process.exit(1);
  });
}
