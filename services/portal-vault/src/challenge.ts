// Spec 3.1, R2, R3: the challenge store. Ported from the fiat service's
// fiat-challenge.ts (in-memory, bounded, idempotent issue, five minute life,
// global cap), with ONE deliberate difference, rule R2:
//
//   A nonce is consumed only by a request whose signature verified.
//
// Issuance is idempotent, so anyone who knows an address can learn its live
// nonce. If a failed attempt burnt the nonce (as the fiat service does), that
// person could keep the owner locked out by spending it again and again. So
// this store separates looking up (`peek`, never consumes) from spending
// (`consume`); handlers verify the signature between the two. A stranger still
// cannot use the nonce, having no signature. `consume` is atomic, so of two
// verified requests racing on one nonce exactly one wins.
//
// The store holds only what the server chose: nonce, address, chain, purpose,
// expiry. The vault hash and the version replaced are NOT stored (spec 3.1);
// they are bound by the signed message instead.
//
// v0.2 (R7): each challenge is charged to the client that caused it to be
// created, and a client may hold at most 20 live ones, so one client cannot
// fill the global cap. A client asking for a challenge that is already live is
// handed it and charged nothing. There is NO limit keyed on the account
// address alone: a stranger could use one to lock an owner out. Expired
// challenges are released from the oldest end (every challenge has the same
// life, so the table is in expiry order); a call never sweeps the whole table.
import { randomBytes } from 'node:crypto';
import { isPurpose, type Purpose } from './message.js';

export const CHALLENGE_TTL_MS = 5 * 60 * 1000;

export interface Challenge {
  /** 48 lower-case hex (24 random bytes). */
  nonce: string;
  /** Lower-case address the challenge was issued for. */
  address: string;
  chainId: number;
  purpose: Purpose;
  /** Epoch milliseconds. */
  expiresAt: number;
}

/** Thrown when the global cap on live challenges is reached (429). */
export class ChallengeStoreFullError extends Error {
  constructor() {
    super('too many live challenges');
    this.name = 'ChallengeStoreFullError';
  }
}

/** Thrown when one client already holds its full share of live challenges (429). */
export class ChallengeClientLimitError extends Error {
  constructor() {
    super('too many live challenges for this client');
    this.name = 'ChallengeClientLimitError';
  }
}

const ADDRESS_RE = /^0x[0-9a-fA-F]{40}$/;
export const CHALLENGES_PER_CLIENT = 20;

export interface ChallengeStoreOptions {
  now: () => number;
  ttlMs?: number;
  /** Global cap on live challenges. */
  max: number;
  /** Cap on live challenges created by one client (default 20). */
  perClientMax?: number;
}

interface Entry {
  challenge: Challenge;
  /** The client charged for it. Never returned to a caller. */
  client: string;
}

export class ChallengeStore {
  // Insertion order is issue order, and every challenge has the same life, so
  // it is also expiry order.
  private readonly byNonce = new Map<string, Entry>();
  private readonly bySlot = new Map<string, string>(); // slot -> nonce
  private readonly perClient = new Map<string, number>(); // client -> live count
  private readonly now: () => number;
  private readonly ttlMs: number;
  private readonly max: number;
  private readonly perClientMax: number;
  /** Entries looked at while releasing expired ones (tests bound the work). */
  examined = 0;

  constructor(opts: ChallengeStoreOptions) {
    if (!Number.isSafeInteger(opts.max) || opts.max <= 0) throw new Error('challenge cap must be a positive integer');
    const ttlMs = opts.ttlMs ?? CHALLENGE_TTL_MS;
    if (!Number.isSafeInteger(ttlMs) || ttlMs <= 0) throw new Error('challenge life must be a positive integer');
    const perClientMax = opts.perClientMax ?? CHALLENGES_PER_CLIENT;
    if (!Number.isSafeInteger(perClientMax) || perClientMax <= 0) throw new Error('per-client challenge cap must be a positive integer');
    this.now = opts.now;
    this.ttlMs = ttlMs;
    this.max = opts.max;
    this.perClientMax = perClientMax;
  }

  private static slot(address: string, chainId: number, purpose: Purpose): string {
    return `${chainId}|${address}|${purpose}`;
  }

  private drop(entry: Entry): void {
    const c = entry.challenge;
    if (!this.byNonce.delete(c.nonce)) return;
    const slot = ChallengeStore.slot(c.address, c.chainId, c.purpose);
    if (this.bySlot.get(slot) === c.nonce) this.bySlot.delete(slot);
    const left = (this.perClient.get(entry.client) ?? 1) - 1;
    if (left <= 0) this.perClient.delete(entry.client);
    else this.perClient.set(entry.client, left);
  }

  /** Release expired challenges from the oldest end; stops at the first live
   *  one, so the cost is the number released plus one. Returns how many went. */
  sweep(): number {
    const t = this.now();
    let released = 0;
    for (const entry of this.byNonce.values()) {
      this.examined += 1;
      if (entry.challenge.expiresAt > t) break;
      this.drop(entry);
      released += 1;
    }
    return released;
  }

  /** Sweep on a timer that does not keep the process alive. */
  startSweeping(intervalMs: number = 30_000): NodeJS.Timeout {
    const timer = setInterval(() => this.sweep(), intervalMs);
    timer.unref();
    return timer;
  }

  /** The live entry for a nonce, or undefined. An expired one is released here,
   *  whether or not the sweep has reached it yet. */
  private live(nonce: string): Entry | undefined {
    const entry = this.byNonce.get(nonce);
    if (!entry) return undefined;
    if (entry.challenge.expiresAt <= this.now()) {
      this.drop(entry);
      return undefined;
    }
    return entry;
  }

  /** Issue a challenge for (address, chainId, purpose), charged to `client`.
   *  Idempotent within its life: a live challenge for the slot is handed back
   *  unchanged, never replaced, to any client and at no charge, so nobody can
   *  evict an owner's in-flight nonce. Throws ChallengeClientLimitError when
   *  the client already holds its share, ChallengeStoreFullError past the
   *  global cap, and a plain Error on a malformed argument. */
  issue(address: string, chainId: number, purpose: Purpose, client: string = 'unknown'): Challenge {
    if (typeof address !== 'string' || !ADDRESS_RE.test(address)) throw new Error('address must be a 20-byte hex address');
    if (!Number.isSafeInteger(chainId) || chainId <= 0) throw new Error('chain id must be a positive integer');
    if (!isPurpose(purpose)) throw new Error('unknown purpose');
    if (typeof client !== 'string' || client.length === 0) throw new Error('client key must be a non-empty string');
    const addr = address.toLowerCase();

    this.sweep();
    const slot = ChallengeStore.slot(addr, chainId, purpose);
    const liveNonce = this.bySlot.get(slot);
    if (liveNonce !== undefined) {
      const entry = this.live(liveNonce);
      if (entry) return { ...entry.challenge };
    }
    if ((this.perClient.get(client) ?? 0) >= this.perClientMax) throw new ChallengeClientLimitError();
    if (this.byNonce.size >= this.max) throw new ChallengeStoreFullError();

    const challenge: Challenge = {
      nonce: randomBytes(24).toString('hex'),
      address: addr,
      chainId,
      purpose,
      expiresAt: this.now() + this.ttlMs,
    };
    this.byNonce.set(challenge.nonce, { challenge, client });
    this.bySlot.set(slot, challenge.nonce);
    this.perClient.set(client, (this.perClient.get(client) ?? 0) + 1);
    return { ...challenge };
  }

  /** Look a nonce up WITHOUT consuming it. Null if unknown, expired or used. */
  peek(nonce: unknown): Challenge | null {
    if (typeof nonce !== 'string' || nonce.length === 0) return null;
    const entry = this.live(nonce);
    return entry ? { ...entry.challenge } : null;
  }

  /** Spend a nonce: returns the challenge exactly once, null afterwards. Call
   *  this only after the signature has verified (R2). */
  consume(nonce: unknown): Challenge | null {
    if (typeof nonce !== 'string' || nonce.length === 0) return null;
    const entry = this.live(nonce);
    if (!entry) return null;
    this.drop(entry);
    return { ...entry.challenge };
  }

  size(): number {
    return this.byNonce.size;
  }
}
