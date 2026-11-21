// R5: the read token.
//
//   base64url(payload) + "." + base64url(HMAC-SHA256(secret, payload))
//   payload = v1|<region>|<portal host>|<address>|<chainId>|<epoch>|<expiry in seconds>
//
// v0.2: region and portal host are in the payload, so a token can never be
// replayed on another portal even if a secret were reused.
//
// Valid when the MAC matches (constant-time compare), it has not expired, it
// was minted by this region and portal host, and the address and chain match
// the request. The fourth condition, that the epoch
// equals the stored record's, needs the record, so the caller checks it with
// `epochIsCurrent` once the record is loaded. Every replace increases the
// epoch, which revokes every token issued before it.
import { createHmac, timingSafeEqual } from 'node:crypto';

export const MIN_SECRET_BYTES = 32;

export interface ReadTokenClaims {
  region: string;
  /** Portal host. */
  portal: string;
  /** 0x + 40 lower-case hex. */
  address: string;
  chainId: number;
  epoch: number;
  expiresAtSeconds: number;
}

const ADDRESS_ANY_RE = /^0x[0-9a-fA-F]{40}$/;
// Region and host: printable ASCII without spaces and without the separator.
const PAYLOAD_RE = /^v1\|([\x21-\x7b\x7d\x7e]{1,64})\|([\x21-\x7b\x7d\x7e]{1,259})\|(0x[0-9a-f]{40})\|([1-9][0-9]{0,15})\|(0|[1-9][0-9]{0,15})\|([1-9][0-9]{0,15})$/;
const BASE64URL_RE = /^[A-Za-z0-9_-]+$/;
const MAX_TOKEN_LENGTH = 1024;

function mac(secret: Buffer, payload: string): Buffer {
  return createHmac('sha256', secret).update(payload, 'utf8').digest();
}

function assertSecret(secret: Buffer): void {
  if (!Buffer.isBuffer(secret) || secret.length < MIN_SECRET_BYTES) {
    throw new Error(`token secret must be at least ${MIN_SECRET_BYTES} bytes`);
  }
}

export function issueReadToken(secret: Buffer, claims: ReadTokenClaims): string {
  assertSecret(secret);
  const payload = `v1|${claims.region}|${claims.portal}|${claims.address}|${claims.chainId}|${claims.epoch}|${claims.expiresAtSeconds}`;
  // Number formatting (exponents, signs, fractions) and address casing are all
  // caught by the one pattern the verifier uses.
  if (
    typeof claims.region !== 'string' ||
    typeof claims.portal !== 'string' ||
    !Number.isSafeInteger(claims.chainId) ||
    !Number.isSafeInteger(claims.epoch) ||
    !Number.isSafeInteger(claims.expiresAtSeconds) ||
    !PAYLOAD_RE.test(payload)
  ) {
    throw new Error('read token claims are malformed');
  }
  return Buffer.from(payload, 'utf8').toString('base64url') + '.' + mac(secret, payload).toString('base64url');
}

export interface ReadTokenExpectation {
  /** This service's own region and portal host. */
  region: string;
  portal: string;
  /** The address in the request (any casing). */
  address: string;
  chainId: number;
  nowSeconds: number;
}

/** Returns the claims of a token that is authentic, unexpired and minted by
 *  this region and portal host for this address and chain; null otherwise. Never throws on bad input. The
 *  caller must still compare the epoch with the stored record's. */
export function verifyReadToken(secret: Buffer, token: unknown, expected: ReadTokenExpectation): ReadTokenClaims | null {
  assertSecret(secret);
  if (typeof token !== 'string' || token.length === 0 || token.length > MAX_TOKEN_LENGTH) return null;
  const parts = token.split('.');
  if (parts.length !== 2) return null;
  const [payloadPart, macPart] = parts as [string, string];
  if (!BASE64URL_RE.test(payloadPart) || !BASE64URL_RE.test(macPart)) return null;

  const payloadBytes = Buffer.from(payloadPart, 'base64url');
  const given = Buffer.from(macPart, 'base64url');
  // One canonical encoding only: a second spelling of the same token is refused.
  if (payloadBytes.toString('base64url') !== payloadPart || given.toString('base64url') !== macPart) return null;
  const payload = payloadBytes.toString('utf8');

  const wanted = mac(secret, payload);
  if (given.length !== wanted.length) return null;
  if (!timingSafeEqual(given, wanted)) return null;

  // Authentic from here on: the payload is one this service wrote.
  const m = PAYLOAD_RE.exec(payload);
  if (!m) return null;
  const claims: ReadTokenClaims = {
    region: m[1]!,
    portal: m[2]!,
    address: m[3]!,
    chainId: Number(m[4]),
    epoch: Number(m[5]),
    expiresAtSeconds: Number(m[6]),
  };
  if (!Number.isSafeInteger(claims.chainId) || !Number.isSafeInteger(claims.epoch) || !Number.isSafeInteger(claims.expiresAtSeconds)) {
    return null;
  }
  if (!Number.isFinite(expected.nowSeconds) || claims.expiresAtSeconds <= expected.nowSeconds) return null;
  if (claims.region !== expected.region) return null;
  if (claims.portal !== expected.portal) return null;
  if (typeof expected.address !== 'string' || !ADDRESS_ANY_RE.test(expected.address)) return null;
  if (claims.address !== expected.address.toLowerCase()) return null;
  if (claims.chainId !== expected.chainId) return null;
  return claims;
}

/** The last condition of R5: the token's epoch equals the stored record's. */
export function epochIsCurrent(claims: ReadTokenClaims, recordEpoch: number): boolean {
  return Number.isSafeInteger(recordEpoch) && claims.epoch === recordEpoch;
}
