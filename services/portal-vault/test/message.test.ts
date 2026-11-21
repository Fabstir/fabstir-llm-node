// Spec section 4: the signed message is fixed and versioned. These tests pin it
// against literal text, because the UI rebuilds the message and refuses any
// difference: a one-character drift here locks every user out.
import { describe, expect, it } from 'vitest';
import { buildMessage, buildTypedData, type ChallengeFields } from '../src/message.js';

const ADDRESS = '0x1234567890abcdef1234567890abcdef12345678';
const NONCE = '00112233445566778899aabbccddeeff0011223344556677';
const HASH = 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855';

const READ: ChallengeFields = {
  purpose: 'read',
  region: 'uk',
  portal: 'uk.portal.example',
  address: ADDRESS,
  chainId: 84532,
  nonce: NONCE,
  expires: '2026-11-21T06:05:00.000Z',
};

const REPLACE: ChallengeFields = { ...READ, purpose: 'replace', vaultSha256: HASH, replaces: 3 };

describe('buildMessage', () => {
  it('builds the exact read message', () => {
    expect(buildMessage(READ)).toBe(
      'Platformless AI: open your key vault\n' +
        'region: uk\n' +
        'portal: uk.portal.example\n' +
        'address: 0x1234567890abcdef1234567890abcdef12345678\n' +
        'chain: 84532\n' +
        'nonce: 00112233445566778899aabbccddeeff0011223344556677\n' +
        'expires: 2026-11-21T06:05:00.000Z'
    );
  });

  it('builds the exact replace message', () => {
    expect(buildMessage(REPLACE)).toBe(
      'Platformless AI: replace your key vault\n' +
        'region: uk\n' +
        'portal: uk.portal.example\n' +
        'address: 0x1234567890abcdef1234567890abcdef12345678\n' +
        'chain: 84532\n' +
        'vault: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n' +
        'replaces: 3\n' +
        'nonce: 00112233445566778899aabbccddeeff0011223344556677\n' +
        'expires: 2026-11-21T06:05:00.000Z'
    );
  });

  it('writes replaces 0 for a first vault', () => {
    expect(buildMessage({ ...REPLACE, replaces: 0 })).toContain('\nreplaces: 0\n');
  });

  it('has no trailing newline and exactly the expected number of lines', () => {
    expect(buildMessage(READ).split('\n')).toHaveLength(7);
    expect(buildMessage(REPLACE).split('\n')).toHaveLength(9);
    expect(buildMessage(READ).endsWith('\n')).toBe(false);
  });

  it('takes the first line only from the fixed list: no caller text can reach it', () => {
    for (const purpose of ['delete', 'Read', '', 'constructor', '__proto__', 'toString', 'read\nregion: evil', 7, null, undefined]) {
      expect(() => buildMessage({ ...READ, purpose: purpose as never })).toThrow();
      expect(() => buildTypedData({ ...READ, purpose: purpose as never })).toThrow();
    }
  });

  it('refuses any field that could add or forge a line', () => {
    expect(() => buildMessage({ ...READ, region: 'uk\nportal: evil' })).toThrow();
    expect(() => buildMessage({ ...READ, portal: 'a\nb' })).toThrow();
    expect(() => buildMessage({ ...READ, portal: '' })).toThrow();
    expect(() => buildMessage({ ...READ, address: ADDRESS.toUpperCase() })).toThrow();
    expect(() => buildMessage({ ...READ, address: '0x12' })).toThrow();
    expect(() => buildMessage({ ...READ, nonce: NONCE.toUpperCase() })).toThrow();
    expect(() => buildMessage({ ...READ, nonce: NONCE + '00' })).toThrow();
    expect(() => buildMessage({ ...READ, chainId: 1.5 })).toThrow();
    expect(() => buildMessage({ ...READ, chainId: -1 })).toThrow();
    expect(() => buildMessage({ ...READ, expires: '2026-11-21T06:05:00Z' })).toThrow();
    expect(() => buildMessage({ ...READ, expires: 'soon' })).toThrow();
  });

  it('requires the hash and the version for a replace and forbids them for a read', () => {
    expect(() => buildMessage({ ...READ, purpose: 'replace' })).toThrow();
    expect(() => buildMessage({ ...REPLACE, vaultSha256: undefined })).toThrow();
    expect(() => buildMessage({ ...REPLACE, replaces: undefined })).toThrow();
    expect(() => buildMessage({ ...REPLACE, vaultSha256: HASH.toUpperCase() })).toThrow();
    expect(() => buildMessage({ ...REPLACE, vaultSha256: HASH.slice(2) })).toThrow();
    expect(() => buildMessage({ ...REPLACE, replaces: -1 })).toThrow();
    expect(() => buildMessage({ ...REPLACE, replaces: 1.5 })).toThrow();
    expect(() => buildMessage({ ...REPLACE, replaces: '1' as never })).toThrow();
    expect(() => buildMessage({ ...READ, vaultSha256: HASH })).toThrow();
    expect(() => buildMessage({ ...READ, replaces: 0 })).toThrow();
  });
});

describe('buildTypedData', () => {
  it('builds the read typed data', () => {
    expect(buildTypedData(READ)).toEqual({
      domain: { name: 'Platformless AI Vault', version: '1', chainId: 84532 },
      types: {
        VaultRead: [
          { name: 'purpose', type: 'string' },
          { name: 'region', type: 'string' },
          { name: 'portal', type: 'string' },
          { name: 'wallet', type: 'address' },
          { name: 'nonce', type: 'string' },
          { name: 'expires', type: 'string' },
        ],
      },
      primaryType: 'VaultRead',
      message: {
        purpose: 'open your key vault',
        region: 'uk',
        portal: 'uk.portal.example',
        wallet: ADDRESS,
        nonce: NONCE,
        expires: '2026-11-21T06:05:00.000Z',
      },
    });
  });

  it('builds the replace typed data, with the version as a decimal string', () => {
    expect(buildTypedData(REPLACE)).toEqual({
      domain: { name: 'Platformless AI Vault', version: '1', chainId: 84532 },
      types: {
        VaultReplace: [
          { name: 'purpose', type: 'string' },
          { name: 'region', type: 'string' },
          { name: 'portal', type: 'string' },
          { name: 'wallet', type: 'address' },
          { name: 'vault', type: 'string' },
          { name: 'replaces', type: 'string' },
          { name: 'nonce', type: 'string' },
          { name: 'expires', type: 'string' },
        ],
      },
      primaryType: 'VaultReplace',
      message: {
        purpose: 'replace your key vault',
        region: 'uk',
        portal: 'uk.portal.example',
        wallet: ADDRESS,
        vault: HASH,
        replaces: '3',
        nonce: NONCE,
        expires: '2026-11-21T06:05:00.000Z',
      },
    });
  });

  it('keeps the field order of the type (it changes the EIP-712 hash)', () => {
    expect(buildTypedData(READ).types.VaultRead!.map((f) => f.name)).toEqual([
      'purpose', 'region', 'portal', 'wallet', 'nonce', 'expires',
    ]);
    expect(buildTypedData(REPLACE).types.VaultReplace!.map((f) => f.name)).toEqual([
      'purpose', 'region', 'portal', 'wallet', 'vault', 'replaces', 'nonce', 'expires',
    ]);
  });

  it('applies the same field checks as the message', () => {
    expect(() => buildTypedData({ ...READ, address: ADDRESS.toUpperCase() })).toThrow();
    expect(() => buildTypedData({ ...READ, vaultSha256: HASH })).toThrow();
    expect(() => buildTypedData({ ...REPLACE, replaces: undefined })).toThrow();
  });
});
