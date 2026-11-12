// TX1.1 — Checkout Sessions are rationed: at most N an hour per account
// (default 5) AND at most M an hour per client address (default 10). A buyer
// outside the country we sell to is refunded by the webhook's gate, and each
// such refund costs us Stripe's processing fee, so starting checkouts is
// bounded. The purchase route is unauthenticated (anyone may top up any
// account), so the account alone is not enough: one client could rotate
// account addresses, or use up someone else's hourly budget. In memory, on
// globalThis (one process, single instance, like the challenge store): a
// restart forgets the window, which only ever loosens it.

const HOUR_MS = 60 * 60 * 1000;

const g = globalThis as typeof globalThis & { __fiatCheckoutStarts?: Map<string, number[]> };

function starts(): Map<string, number[]> {
  g.__fiatCheckoutStarts ??= new Map();
  return g.__fiatCheckoutStarts;
}

function positiveIntEnv(name: string, fallback: number): number {
  const raw = process.env[name];
  const n = raw ? Number(raw) : fallback;
  return Number.isInteger(n) && n > 0 ? n : fallback;
}

export function maxCheckoutsPerHour(): number {
  return positiveIntEnv('FIAT_MAX_CHECKOUTS_PER_HOUR', 5);
}

export function maxCheckoutsPerHourPerIp(): number {
  return positiveIntEnv('FIAT_MAX_CHECKOUTS_PER_HOUR_PER_IP', 10);
}

const LOOPBACK = /^(127\.|::1$|::ffff:127\.|localhost$)/i;

/**
 * The client address for rate limiting, or undefined when there is none worth
 * keying on. The service listens on loopback behind nginx on the same box,
 * and nginx APPENDS the address it saw to X-Forwarded-For
 * (`$proxy_add_x_forwarded_for`): so the RIGHTMOST entry is ours, and every
 * entry to its left came from the client and is never trusted. A route handler
 * cannot see the socket; when no proxy set the header, Next fills it with the
 * peer's address, which is then the rightmost entry too. A loopback value
 * (local development, or a caller on the box) is not a client: no IP key.
 */
export function clientIp(req: Request): string | undefined {
  const header = req.headers.get('x-forwarded-for');
  if (!header) return undefined;
  const last = header.split(',').pop()?.trim();
  if (!last || LOOPBACK.test(last)) return undefined;
  return last.toLowerCase();
}

export type CheckoutSlot = { ok: true; release(): void } | { ok: false; retryAfterS: number; limitedBy: 'account' | 'ip' };

/** Take one checkout start for the account and, when known, the client
 *  address. `release()` gives it back (the Stripe call failed: no session was
 *  created, so nothing was used). */
export function takeCheckoutStart(userId: string, ip?: string, nowMs: number = Date.now()): CheckoutSlot {
  const all = starts();
  const keys: Array<{ key: string; max: number; by: 'account' | 'ip' }> = [
    { key: `u:${userId}`, max: maxCheckoutsPerHour(), by: 'account' },
    ...(ip ? [{ key: `ip:${ip}`, max: maxCheckoutsPerHourPerIp(), by: 'ip' as const }] : []),
  ];
  const windows = keys.map((k) => ({ ...k, recent: (all.get(k.key) ?? []).filter((at) => nowMs - at < HOUR_MS) }));
  for (const w of windows) {
    all.set(w.key, w.recent);
    if (w.recent.length >= w.max) {
      return { ok: false, retryAfterS: Math.max(1, Math.ceil((w.recent[0]! + HOUR_MS - nowMs) / 1000)), limitedBy: w.by };
    }
  }
  for (const w of windows) w.recent.push(nowMs);
  // Bound the map: drop keys whose window has emptied.
  if (all.size > 20_000) {
    for (const [key, times] of all) {
      if (times.every((at) => nowMs - at >= HOUR_MS)) all.delete(key);
    }
  }
  let released = false;
  return {
    ok: true,
    release() {
      if (released) return;
      released = true;
      for (const w of windows) {
        const times = all.get(w.key);
        const i = times ? times.lastIndexOf(nowMs) : -1;
        if (times && i >= 0) times.splice(i, 1);
      }
    },
  };
}

export function resetCheckoutRateLimitForTest(): void {
  g.__fiatCheckoutStarts = new Map();
}
