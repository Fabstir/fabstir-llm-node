// Shared test fixtures. Nothing here prints: vault text, signatures, nonces and
// tokens stay inside the assertions.
import { createHash } from 'node:crypto';
import { ChallengeStore } from '../src/challenge.js';
import type { HandlerConfig, HandlerDeps, LogEntry } from '../src/handlers.js';
import { RateLimiter } from '../src/ratelimit.js';
import { SignatureCheckUnavailableError, type OwnershipProof, type SignatureVerifier } from '../src/signature.js';
import { AccountLocks, MemoryVaultStore } from '../src/store.js';

export const ADDR_A = '0xAbCdef0123456789abcDEF0123456789aBCdEf01';
export const ADDR_A_LC = ADDR_A.toLowerCase();
export const ADDR_B = '0x1111111111111111111111111111111111111111';
export const CHAIN = 84532;
export const START = Date.parse('2026-11-21T06:00:00.000Z');

export const sha256Hex = (text: string) => createHash('sha256').update(text, 'utf8').digest('hex');

function proofText(proof: OwnershipProof): string {
  return 'typedData' in proof ? 'typed:' + JSON.stringify(proof.typedData) : 'message:' + proof.message;
}

/** The fake wallet: a "signature" is a hash of the signer and of exactly what
 *  was signed, so the fake verifier accepts it only for the same signer, the
 *  same form and the same text, as a real one would. */
export function fakeSign(address: string, signed: { message: string } | { typedData: unknown }): string {
  const text = 'typedData' in signed ? 'typed:' + JSON.stringify(signed.typedData) : 'message:' + signed.message;
  return '0x' + sha256Hex(address.toLowerCase() + '\n' + text);
}

export interface FakeVerifier {
  verify: SignatureVerifier;
  calls: OwnershipProof[];
  /** 'strict' checks the fake signature; 'unavailable' throws as an RPC outage
   *  would; 'always-true' is the worst case, used to show that a check does not
   *  rest on the signature alone. */
  mode: 'strict' | 'unavailable' | 'always-true';
  delayMs: number;
}

export function fakeVerifier(): FakeVerifier {
  const fake: FakeVerifier = {
    calls: [],
    mode: 'strict',
    delayMs: 0,
    verify: async (proof) => {
      fake.calls.push(proof);
      if (fake.delayMs > 0) await new Promise((r) => setTimeout(r, fake.delayMs));
      if (fake.mode === 'unavailable') throw new SignatureCheckUnavailableError(new Error('rpc down (test)'));
      if (fake.mode === 'always-true') return true;
      return proof.signature === '0x' + sha256Hex(proof.address.toLowerCase() + '\n' + proofText(proof));
    },
  };
  return fake;
}

export const TEST_CONFIG: HandlerConfig = {
  region: 'uk',
  portal: 'uk.portal.example',
  chainId: CHAIN,
  tokenSecret: Buffer.alloc(32, 9),
  readTokenSeconds: 30 * 24 * 3600,
  maxVaultBytes: 32768,
  storageTimeoutMs: 2000,
};

export interface TestEnv {
  deps: HandlerDeps;
  store: MemoryVaultStore;
  challenges: ChallengeStore;
  verifier: FakeVerifier;
  clock: { t: number };
  logs: LogEntry[];
}

export function makeEnv(
  over: { config?: Partial<HandlerConfig>; challengeMax?: number; verifyPerMinute?: number; wrapVerifier?: (v: SignatureVerifier) => SignatureVerifier } = {}
): TestEnv {
  const clock = { t: START };
  const store = new MemoryVaultStore();
  const challenges = new ChallengeStore({ now: () => clock.t, max: over.challengeMax ?? 1000 });
  const verifier = fakeVerifier();
  const logs: LogEntry[] = [];
  const deps: HandlerDeps = {
    config: { ...TEST_CONFIG, ...over.config },
    store,
    challenges,
    verify: over.wrapVerifier ? over.wrapVerifier(verifier.verify) : verifier.verify,
    verifyLimiter: new RateLimiter({ perMinute: over.verifyPerMinute ?? 100_000, now: () => clock.t }),
    now: () => clock.t,
    log: (entry) => logs.push(entry),
    locks: new AccountLocks(),
  };
  return { deps, store, challenges, verifier, clock, logs };
}
