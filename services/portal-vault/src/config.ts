// R10: configuration fails closed. The service refuses to start unless region,
// portal host, chain id, at least one RPC URL, the token secret (at least 32
// bytes) and the full S3 settings are present. There is NO default for any of
// them. A malformed optional value also refuses to start, instead of falling
// back silently. Problems name the variable, never its value.
import { isIP } from 'node:net';

export const REQUIRED_VARIABLES = [
  'VAULT_REGION',
  'VAULT_PORTAL_HOST',
  'VAULT_CHAIN_ID',
  'VAULT_RPC_URLS',
  'VAULT_TOKEN_SECRET',
  'VAULT_S3_ENDPOINT',
  'VAULT_S3_ACCESS_KEY',
  'VAULT_S3_SECRET_KEY',
  'VAULT_S3_BUCKET',
  'VAULT_S3_REGION',
] as const;

/** Chains the signature check knows (must match src/signature.ts; kept here as
 *  plain numbers so this module does not load viem). */
const KNOWN_CHAIN_IDS = [84532, 8453];

/** R7: the request body limit. Fixed by the specification, not configurable. */
export const MAX_BODY_BYTES = 64 * 1024;

export interface Config {
  region: string;
  portalHost: string;
  chainId: number;
  rpcUrls: string[];
  tokenSecret: Buffer;
  s3: { endpoint: string; accessKey: string; secretKey: string; bucket: string; region: string };
  bind: string;
  port: number;
  readTokenDays: number;
  maxVaultBytes: number;
  allowedOrigins: string[];
  ratePerMin: number;
  challengeMax: number;
}

export class ConfigError extends Error {
  readonly problems: string[];
  constructor(problems: string[]) {
    super('configuration refused:\n' + problems.map((p) => `  - ${p}`).join('\n'));
    this.name = 'ConfigError';
    this.problems = problems;
  }
}

const REGION_RE = /^[a-z0-9][a-z0-9-]{0,31}$/;
const HOST_RE = /^[a-z0-9]([a-z0-9.-]{0,251}[a-z0-9])?(:[0-9]{1,5})?$/;
const DECIMAL_RE = /^[1-9][0-9]{0,9}$/;
const HEX_RE = /^([0-9a-fA-F]{2})+$/;

function isHttpUrl(text: string): boolean {
  try {
    const u = new URL(text);
    return (u.protocol === 'http:' || u.protocol === 'https:') && u.hostname.length > 0;
  } catch {
    return false;
  }
}

/** R10 v0.2: 127.0.0.0/8, ::1 and localhost. Traffic to anything else leaves
 *  the box and must be encrypted. */
function isLoopbackHost(hostname: string): boolean {
  const host = hostname.startsWith('[') && hostname.endsWith(']') ? hostname.slice(1, -1) : hostname;
  if (host === 'localhost' || host === '::1') return true;
  return isIP(host) === 4 && host.startsWith('127.');
}

/** An http or https URL, and https unless its host is loopback. */
function isSafeEndpoint(text: string): boolean {
  if (!isHttpUrl(text)) return false;
  const u = new URL(text);
  return u.protocol === 'https:' || isLoopbackHost(u.hostname);
}

const MIN_DISTINCT_SECRET_BYTES = 8;

function isOrigin(text: string): boolean {
  try {
    const u = new URL(text);
    return (u.protocol === 'http:' || u.protocol === 'https:') && u.origin === text;
  } catch {
    return false;
  }
}

export function loadConfig(env: Record<string, string | undefined>): Config {
  const problems: string[] = [];
  const read = (name: string): string | undefined => {
    const value = env[name];
    if (typeof value !== 'string') return undefined;
    const trimmed = value.trim();
    return trimmed.length === 0 ? undefined : trimmed;
  };
  const required = (name: (typeof REQUIRED_VARIABLES)[number]): string | undefined => {
    const value = read(name);
    if (value === undefined) problems.push(`${name} is required and is not set`);
    return value;
  };
  const optionalInt = (name: string, fallback: number, min: number, max: number): number => {
    const value = read(name);
    if (value === undefined) return fallback;
    const n = DECIMAL_RE.test(value) ? Number(value) : NaN;
    if (!Number.isSafeInteger(n) || n < min || n > max) {
      problems.push(`${name} must be a whole number from ${min} to ${max}`);
      return fallback;
    }
    return n;
  };

  const region = required('VAULT_REGION');
  if (region !== undefined && !REGION_RE.test(region)) {
    problems.push('VAULT_REGION must be 1 to 32 lower-case letters, digits or hyphens');
  }

  const portalHost = required('VAULT_PORTAL_HOST');
  if (portalHost !== undefined && !HOST_RE.test(portalHost)) {
    problems.push('VAULT_PORTAL_HOST must be a lower-case host name (optionally with a port), with no scheme and no path');
  }

  const chainIdText = required('VAULT_CHAIN_ID');
  let chainId = 0;
  if (chainIdText !== undefined) {
    chainId = DECIMAL_RE.test(chainIdText) ? Number(chainIdText) : NaN;
    if (!KNOWN_CHAIN_IDS.includes(chainId)) {
      problems.push(`VAULT_CHAIN_ID must be one of ${KNOWN_CHAIN_IDS.join(', ')}`);
    }
  }

  const rpcText = required('VAULT_RPC_URLS');
  let rpcUrls: string[] = [];
  if (rpcText !== undefined) {
    rpcUrls = rpcText.split(',').map((u) => u.trim());
    if (rpcUrls.length === 0 || rpcUrls.some((u) => !isSafeEndpoint(u))) {
      problems.push('VAULT_RPC_URLS must be a comma list of one or more https URLs (http only for a loopback host)');
    }
  }

  const secretText = required('VAULT_TOKEN_SECRET');
  let tokenSecret = Buffer.alloc(0);
  if (secretText !== undefined) {
    if (!HEX_RE.test(secretText) || secretText.length < 64) {
      problems.push('VAULT_TOKEN_SECRET must be hex and at least 32 bytes (64 hex characters)');
    } else {
      tokenSecret = Buffer.from(secretText, 'hex');
      // A secret typed by hand or left as a placeholder ("00..", "a1a1..") is
      // refused. This is a floor, not a measure of randomness.
      if (new Set(tokenSecret).size < MIN_DISTINCT_SECRET_BYTES) {
        problems.push(`VAULT_TOKEN_SECRET must be random: it has fewer than ${MIN_DISTINCT_SECRET_BYTES} distinct byte values`);
        tokenSecret = Buffer.alloc(0);
      }
    }
  }

  const s3Endpoint = required('VAULT_S3_ENDPOINT');
  if (s3Endpoint !== undefined && !isSafeEndpoint(s3Endpoint)) {
    problems.push('VAULT_S3_ENDPOINT must be an https URL (http only for a loopback host)');
  }
  const s3AccessKey = required('VAULT_S3_ACCESS_KEY');
  const s3SecretKey = required('VAULT_S3_SECRET_KEY');
  const s3Bucket = required('VAULT_S3_BUCKET');
  const s3Region = required('VAULT_S3_REGION');

  const bind = read('VAULT_BIND') ?? '127.0.0.1';
  if (isIP(bind) === 0) problems.push('VAULT_BIND must be an IP address');
  const port = optionalInt('VAULT_PORT', 5052, 1, 65535);
  const readTokenDays = optionalInt('VAULT_READ_TOKEN_DAYS', 30, 1, 3650);
  // A vault larger than the request body limit could never be sent.
  const maxVaultBytes = optionalInt('VAULT_MAX_BYTES', 32768, 1, MAX_BODY_BYTES);
  const ratePerMin = optionalInt('VAULT_RATE_PER_MIN', 60, 1, 1_000_000);
  const challengeMax = optionalInt('VAULT_CHALLENGE_MAX', 10000, 1, 10_000_000);

  let allowedOrigins: string[] = [];
  const originsText = read('VAULT_ALLOWED_ORIGINS');
  if (originsText !== undefined) {
    allowedOrigins = originsText.split(',').map((o) => o.trim());
    if (allowedOrigins.some((o) => !isOrigin(o))) {
      problems.push('VAULT_ALLOWED_ORIGINS must be a comma list of exact origins such as https://app.example (no path, no wildcard)');
    }
  }

  if (problems.length > 0) throw new ConfigError(problems);
  return {
    region: region!,
    portalHost: portalHost!,
    chainId,
    rpcUrls,
    tokenSecret,
    s3: { endpoint: s3Endpoint!, accessKey: s3AccessKey!, secretKey: s3SecretKey!, bucket: s3Bucket!, region: s3Region! },
    bind,
    port,
    readTokenDays,
    maxVaultBytes,
    allowedOrigins,
    ratePerMin,
    challengeMax,
  };
}
