// R7: requests per client per minute. A fixed one-minute window per client, an
// injected clock, and bounded memory.
//
// v0.2: a client is an IPv4 address or an IPv6 /64 (`clientKey`); expired
// entries are released by a timer (`startSweeping`), and a call never sweeps
// the whole table. Every window has the same length, so the table, kept in the
// order the windows were opened, is also in the order they expire: a call only
// looks at the oldest entries and stops at the first live one.
import { isIP } from 'node:net';

/** The key a client is counted by: an IPv4 address as it is; an IPv6 address
 *  as its /64 (one subscriber holds a whole /64, so counting single addresses
 *  would be no limit at all); an IPv4-mapped IPv6 address as the IPv4 address.
 *  Anything that is not an address becomes the one key "unknown". */
export function clientKey(address: string): string {
  if (typeof address !== 'string') return 'unknown';
  const zoneAt = address.indexOf('%');
  const bare = zoneAt === -1 ? address : address.slice(0, zoneAt);
  const family = isIP(bare);
  if (family === 4) return bare;
  if (family !== 6) return 'unknown';

  // Expand to eight groups.
  let text = bare.toLowerCase();
  const lastColon = text.lastIndexOf(':');
  const tail = text.slice(lastColon + 1);
  if (tail.includes('.')) {
    // Dotted IPv4 tail: rewrite as two hex groups.
    const o = tail.split('.').map(Number);
    text = text.slice(0, lastColon + 1) + ((o[0]! << 8) | o[1]!).toString(16) + ':' + ((o[2]! << 8) | o[3]!).toString(16);
  }
  const halves = text.split('::');
  const head = halves[0] ? halves[0].split(':') : [];
  const rest = halves.length > 1 && halves[1] ? halves[1].split(':') : [];
  const groups = halves.length > 1 ? [...head, ...new Array<string>(8 - head.length - rest.length).fill('0'), ...rest] : head;
  if (groups.length !== 8) return 'unknown';
  const g = groups.map((x) => parseInt(x, 16));
  if (g.some((n) => !Number.isInteger(n) || n < 0 || n > 0xffff)) return 'unknown';

  // IPv4-mapped (::ffff:a.b.c.d): the client is the IPv4 address.
  if (g[0] === 0 && g[1] === 0 && g[2] === 0 && g[3] === 0 && g[4] === 0 && g[5] === 0xffff) {
    return `${g[6]! >> 8}.${g[6]! & 0xff}.${g[7]! >> 8}.${g[7]! & 0xff}`;
  }
  return `${g[0]!.toString(16)}:${g[1]!.toString(16)}:${g[2]!.toString(16)}:${g[3]!.toString(16)}::/64`;
}

export interface RateLimiterOptions {
  perMinute: number;
  now: () => number;
  /** Most clients remembered at once. Past it, clients not yet known are
   *  refused until a window passes, so memory cannot be grown by a caller. */
  maxClients?: number;
}

export interface RateDecision {
  allowed: boolean;
  /** Whole seconds until the client may try again (0 when allowed). */
  retryAfterSeconds: number;
}

const WINDOW_MS = 60_000;

export class RateLimiter {
  // Insertion order is the order the windows were opened, so also expiry order.
  private readonly windows = new Map<string, { start: number; count: number }>();
  private readonly perMinute: number;
  private readonly now: () => number;
  private readonly maxClients: number;
  /** Entries looked at while releasing expired ones (tests bound the work). */
  examined = 0;

  constructor(opts: RateLimiterOptions) {
    if (!Number.isSafeInteger(opts.perMinute) || opts.perMinute <= 0) throw new Error('rate limit must be a positive integer');
    const maxClients = opts.maxClients ?? 100_000;
    if (!Number.isSafeInteger(maxClients) || maxClients <= 0) throw new Error('client cap must be a positive integer');
    this.perMinute = opts.perMinute;
    this.now = opts.now;
    this.maxClients = maxClients;
  }

  /** Release expired windows from the oldest end; stops at the first live one,
   *  so the cost is the number released plus one. Returns how many went. */
  sweep(): number {
    const t = this.now();
    let released = 0;
    for (const [client, w] of this.windows) {
      this.examined += 1;
      if (t - w.start < WINDOW_MS) break;
      this.windows.delete(client);
      released += 1;
    }
    return released;
  }

  /** Sweep on a timer that does not keep the process alive. */
  startSweeping(intervalMs: number = WINDOW_MS): NodeJS.Timeout {
    const timer = setInterval(() => this.sweep(), intervalMs);
    timer.unref();
    return timer;
  }

  check(client: string): RateDecision {
    const t = this.now();
    let w = this.windows.get(client);
    if (w && t - w.start >= WINDOW_MS) {
      this.windows.delete(client);
      w = undefined;
    }
    if (!w) {
      // Only when the table is full is anything else looked at, and then only
      // the oldest entries: never the whole table.
      if (this.windows.size >= this.maxClients) this.sweep();
      if (this.windows.size >= this.maxClients) return { allowed: false, retryAfterSeconds: 60 };
      this.windows.set(client, { start: t, count: 1 });
      return { allowed: true, retryAfterSeconds: 0 };
    }
    if (w.count < this.perMinute) {
      w.count += 1;
      return { allowed: true, retryAfterSeconds: 0 };
    }
    const left = Math.ceil((w.start + WINDOW_MS - t) / 1000);
    return { allowed: false, retryAfterSeconds: Math.min(60, Math.max(1, left)) };
  }

  size(): number {
    return this.windows.size;
  }
}
