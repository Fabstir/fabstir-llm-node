// FC2.2 — the self-serve challenge store (Decision 1 + Decision 9). A user proves
// they control their smart-account address by signing a fresh, single-use,
// server-issued challenge; the signature is then verified on-chain (FC2.1) and,
// only on success, a spending credential is minted.
//
// The store is in-memory and DoS-bounded (Decision 9): ONE outstanding nonce per
// address, issued IDEMPOTENTLY (a second challenge within the TTL RETURNS the live
// one rather than replacing it — so an attacker who knows a victim's public
// address can't evict the victim's in-flight nonce), a 5-minute TTL with an expiry
// sweep, and a global cap beyond which NEW issuance refuses (→ 429). A nonce is
// CONSUMED on any mint attempt (success or failure), so one captured nonce can
// never be brute-forced or replayed.
//
// Method-agnostic (R4): the same challenge fields produce BOTH a human-legible
// string (personal_sign) and an equivalent EIP-712 payload (eth_signTypedData_v4)
// via shared builders, so the browser can sign either and the route verifies the
// matching form — no dependency on the R1 spike's outcome.
import { randomBytes } from 'node:crypto';
import { fiatUserId } from './fiat-identity';
import { BASE_SEPOLIA_CHAIN_ID } from './seed';

const INTENT = 'authorise card-paid rendering';
const DOMAIN_PREFIX = `Platformless AI — ${INTENT}`;
const DEFAULT_TTL_MS = 5 * 60 * 1000; // 5 minutes

export interface Challenge {
  nonce: string;
  /** Lowercased fc1UserId — the SERVER's record of who the challenge is for. */
  address: string;
  message: string;
  expiresAt: number; // epoch ms
}

/** The human-legible string shown in the passkey prompt (personal_sign form).
 *  Binds the fixed intent, the lowercased address, the nonce, and the expiry. */
export function buildChallengeMessage(c: { address: string; nonce: string; expiresAt: number }): string {
  return `${DOMAIN_PREFIX}\naddress: ${c.address}\nnonce: ${c.nonce}\nexpires: ${new Date(c.expiresAt).toISOString()}`;
}

const CHALLENGE_TYPES = {
  Ownership: [
    { name: 'intent', type: 'string' },
    { name: 'wallet', type: 'address' },
    { name: 'nonce', type: 'string' },
    { name: 'expires', type: 'string' },
  ],
} as const;

/** The EIP-712 equivalent of the string message (same fields), for providers
 *  that expose eth_signTypedData_v4 rather than personal_sign. */
export function buildChallengeTypedData(c: { address: string; nonce: string; expiresAt: number }) {
  return {
    domain: { name: 'Platformless AI', version: '1', chainId: BASE_SEPOLIA_CHAIN_ID },
    types: CHALLENGE_TYPES,
    primaryType: 'Ownership',
    message: {
      intent: INTENT,
      wallet: c.address,
      nonce: c.nonce,
      expires: new Date(c.expiresAt).toISOString(),
    },
  } as const;
}

/** Thrown when the global outstanding-challenge cap is hit → route responds 429. */
export class ChallengeStoreFullError extends Error {
  constructor() {
    super('too many outstanding challenges — try again shortly');
    this.name = 'ChallengeStoreFullError';
  }
}

function challengeMaxFromEnv(): number {
  const raw = process.env.FIAT_CHALLENGE_MAX;
  const n = raw ? Number(raw) : 10_000;
  return Number.isInteger(n) && n > 0 ? n : 10_000;
}

export class ChallengeStore {
  private byNonce = new Map<string, Challenge>();
  private byAddress = new Map<string, string>(); // address -> its outstanding nonce

  constructor(
    private readonly now: () => number = Date.now,
    private readonly ttlMs: number = DEFAULT_TTL_MS,
    private readonly max: number = challengeMaxFromEnv()
  ) {}

  private dropExpired(): void {
    const t = this.now();
    for (const [nonce, c] of this.byNonce) {
      if (c.expiresAt <= t) this.drop(nonce, c.address);
    }
  }

  private drop(nonce: string, address: string): void {
    this.byNonce.delete(nonce);
    if (this.byAddress.get(address) === nonce) this.byAddress.delete(address);
  }

  /** Issue a challenge for `address`. IDEMPOTENT within the TTL: if a live
   *  challenge already exists for the address, hand THAT one back rather than
   *  minting a new one and dropping the old. This closes a targeted-eviction
   *  grief (an attacker who knows a victim's public address could otherwise spam
   *  this endpoint during the victim's biometric prompt, replacing the victim's
   *  in-flight nonce so their mint 401s). A nonce is single-use — consumed on
   *  mint — so returning the same live one repeatedly is safe. Throws
   *  ChallengeStoreFullError past the global cap, or on a malformed address. */
  issue(address: string): Challenge {
    const addr = fiatUserId(address); // normalise + validate (throws on malformed)
    this.dropExpired();
    const existingNonce = this.byAddress.get(addr);
    if (existingNonce) {
      const existing = this.byNonce.get(existingNonce);
      if (existing) return existing; // live (dropExpired cleared any expired) — idempotent
    }
    if (this.byNonce.size >= this.max) throw new ChallengeStoreFullError();

    const nonce = randomBytes(24).toString('hex');
    const expiresAt = this.now() + this.ttlMs;
    const message = buildChallengeMessage({ address: addr, nonce, expiresAt });
    const challenge: Challenge = { nonce, address: addr, message, expiresAt };
    this.byNonce.set(nonce, challenge);
    this.byAddress.set(addr, nonce);
    return challenge;
  }

  /** Consume the nonce on ANY attempt (Decision 9): look it up, delete it, and
   *  return the stored challenge — or null if unknown/expired/already used. A
   *  failed verify still burns the nonce because consumption happens HERE, before
   *  verification. */
  consume(nonce: unknown): Challenge | null {
    this.dropExpired();
    if (typeof nonce !== 'string' || nonce.length === 0) return null;
    const challenge = this.byNonce.get(nonce);
    if (!challenge) return null;
    this.drop(nonce, challenge.address);
    if (challenge.expiresAt <= this.now()) return null;
    return challenge;
  }

  /** Test/introspection helper. */
  size(): number {
    return this.byNonce.size;
  }
}

let override: ChallengeStore | undefined;

/** Tests inject a store (e.g. a small-cap or fast-TTL one); undefined restores. */
export function setChallengeStoreForTest(store: ChallengeStore | undefined): void {
  override = store;
}

// Keep the singleton on globalThis, not a module-local `let`. In Next dev a
// route can be recompiled on demand (or HMR fires) between the GET /challenge
// and the POST /self-serve, which would re-evaluate this module and drop a live
// nonce → "challenge unknown". A globalThis-held store survives that. (A FULL
// server restart still clears it — nonces are in-memory by design, R2 — but a
// hot-reload no longer does.)
const globalStore = globalThis as typeof globalThis & { __fiatChallengeStore?: ChallengeStore };

export function getChallengeStore(): ChallengeStore {
  if (override) return override;
  if (!globalStore.__fiatChallengeStore) globalStore.__fiatChallengeStore = new ChallengeStore();
  return globalStore.__fiatChallengeStore;
}
