// Spec section 4: the message a wallet signs. Fixed and versioned (version 1).
//
// The UI rebuilds this text from the separately returned fields and refuses to
// sign anything that differs, so nothing here may change without a new version
// of the specification. Pure: no clock, no configuration, no I/O.
//
// No caller-supplied text ever enters the message. The first line is chosen
// from a fixed list by purpose; every other field is checked against a strict
// shape before it is written, so no field can add or forge a line.

export type Purpose = 'read' | 'replace';

const FIRST_LINE: Readonly<Record<Purpose, string>> = Object.freeze({
  read: 'Platformless AI: open your key vault',
  replace: 'Platformless AI: replace your key vault',
});

const TYPED_PURPOSE: Readonly<Record<Purpose, string>> = Object.freeze({
  read: 'open your key vault',
  replace: 'replace your key vault',
});

export const ADDRESS_LC_RE = /^0x[0-9a-f]{40}$/;
export const NONCE_RE = /^[0-9a-f]{48}$/;
export const SHA256_HEX_RE = /^[0-9a-f]{64}$/;
// ISO 8601 UTC with milliseconds, exactly what Date.prototype.toISOString gives.
const EXPIRES_RE = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/;
// Region and portal host come from configuration, never from a caller. They are
// still checked here: printable ASCII without spaces, so a bad setting cannot
// produce a message with an extra line.
const LABEL_VALUE_RE = /^[\x21-\x7e]{1,253}$/;

export function isPurpose(value: unknown): value is Purpose {
  return value === 'read' || value === 'replace';
}

export interface ChallengeFields {
  purpose: Purpose;
  region: string;
  portal: string;
  /** 0x + 40 lower-case hex. */
  address: string;
  chainId: number;
  /** 48 lower-case hex. */
  nonce: string;
  /** ISO 8601 UTC with milliseconds. */
  expires: string;
  /** Replace only: 64 lower-case hex, SHA-256 of the UTF-8 bytes of the new vault text. */
  vaultSha256?: string;
  /** Replace only: the version being replaced, 0 if none. */
  replaces?: number;
}

interface Checked {
  purpose: Purpose;
  region: string;
  portal: string;
  address: string;
  chainId: number;
  nonce: string;
  expires: string;
  replace: { vaultSha256: string; replaces: number } | null;
}

function fail(field: string): never {
  // Names the field only; never echoes the value.
  throw new Error(`challenge field "${field}" is missing or malformed`);
}

function check(f: ChallengeFields): Checked {
  if (!isPurpose(f.purpose)) fail('purpose');
  if (typeof f.region !== 'string' || !LABEL_VALUE_RE.test(f.region)) fail('region');
  if (typeof f.portal !== 'string' || !LABEL_VALUE_RE.test(f.portal)) fail('portal');
  if (typeof f.address !== 'string' || !ADDRESS_LC_RE.test(f.address)) fail('address');
  if (!Number.isSafeInteger(f.chainId) || f.chainId <= 0) fail('chainId');
  if (typeof f.nonce !== 'string' || !NONCE_RE.test(f.nonce)) fail('nonce');
  if (typeof f.expires !== 'string' || !EXPIRES_RE.test(f.expires)) fail('expires');

  let replace: Checked['replace'] = null;
  if (f.purpose === 'replace') {
    if (typeof f.vaultSha256 !== 'string' || !SHA256_HEX_RE.test(f.vaultSha256)) fail('vaultSha256');
    if (typeof f.replaces !== 'number' || !Number.isSafeInteger(f.replaces) || f.replaces < 0) fail('replaces');
    replace = { vaultSha256: f.vaultSha256, replaces: f.replaces };
  } else {
    if (f.vaultSha256 !== undefined) fail('vaultSha256');
    if (f.replaces !== undefined) fail('replaces');
  }
  return {
    purpose: f.purpose,
    region: f.region,
    portal: f.portal,
    address: f.address,
    chainId: f.chainId,
    nonce: f.nonce,
    expires: f.expires,
    replace,
  };
}

/** The text signed with `personal_sign`. Lines joined with a single "\n", no
 *  trailing newline. Throws on any malformed field. */
export function buildMessage(fields: ChallengeFields): string {
  const c = check(fields);
  const lines = [
    FIRST_LINE[c.purpose],
    `region: ${c.region}`,
    `portal: ${c.portal}`,
    `address: ${c.address}`,
    `chain: ${c.chainId}`,
  ];
  if (c.replace) {
    lines.push(`vault: ${c.replace.vaultSha256}`, `replaces: ${c.replace.replaces}`);
  }
  lines.push(`nonce: ${c.nonce}`, `expires: ${c.expires}`);
  return lines.join('\n');
}

export interface TypedDataField {
  name: string;
  type: string;
}

export interface VaultTypedData {
  domain: { name: string; version: string; chainId: number };
  types: Record<string, TypedDataField[]>;
  primaryType: 'VaultRead' | 'VaultReplace';
  message: Record<string, string>;
}

/** The EIP-712 form of the same challenge, signed with `eth_signTypedData_v4`.
 *  A fresh object on every call, so no caller can alter a shared one. */
export function buildTypedData(fields: ChallengeFields): VaultTypedData {
  const c = check(fields);
  const domain = { name: 'Platformless AI Vault', version: '1', chainId: c.chainId };
  const head: TypedDataField[] = [
    { name: 'purpose', type: 'string' },
    { name: 'region', type: 'string' },
    { name: 'portal', type: 'string' },
    { name: 'wallet', type: 'address' },
  ];
  const tail: TypedDataField[] = [
    { name: 'nonce', type: 'string' },
    { name: 'expires', type: 'string' },
  ];
  if (c.replace) {
    return {
      domain,
      types: {
        VaultReplace: [...head, { name: 'vault', type: 'string' }, { name: 'replaces', type: 'string' }, ...tail],
      },
      primaryType: 'VaultReplace',
      message: {
        purpose: TYPED_PURPOSE[c.purpose],
        region: c.region,
        portal: c.portal,
        wallet: c.address,
        vault: c.replace.vaultSha256,
        replaces: String(c.replace.replaces),
        nonce: c.nonce,
        expires: c.expires,
      },
    };
  }
  return {
    domain,
    types: { VaultRead: [...head, ...tail] },
    primaryType: 'VaultRead',
    message: {
      purpose: TYPED_PURPOSE[c.purpose],
      region: c.region,
      portal: c.portal,
      wallet: c.address,
      nonce: c.nonce,
      expires: c.expires,
    },
  };
}
