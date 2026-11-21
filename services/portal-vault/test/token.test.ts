// R5: the read token.
import { createHmac } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import { epochIsCurrent, issueReadToken, verifyReadToken } from '../src/token.js';

const SECRET = Buffer.alloc(32, 7);
const OTHER_SECRET = Buffer.alloc(32, 8);
const A = '0x1234567890abcdef1234567890abcdef12345678';
const B = '0x1111111111111111111111111111111111111111';
const NOW = 1_800_000_000;
const REGION = 'uk';
const PORTAL = 'uk.portal.example';
const CLAIMS = { region: REGION, portal: PORTAL, address: A, chainId: 84532, epoch: 5, expiresAtSeconds: NOW + 3600 };
const EXPECT = { region: REGION, portal: PORTAL, address: A, chainId: 84532, nowSeconds: NOW };

const b64u = (s: string | Buffer) => Buffer.from(s).toString('base64url');
function forge(payload: string, secret: Buffer = SECRET): string {
  return b64u(payload) + '.' + createHmac('sha256', secret).update(payload).digest('base64url');
}

describe('read token', () => {
  it('has the documented shape', () => {
    const token = issueReadToken(SECRET, CLAIMS);
    const [p, m, ...rest] = token.split('.');
    expect(rest).toHaveLength(0);
    const payload = Buffer.from(p!, 'base64url').toString('utf8');
    expect(payload).toBe(`v1|uk|uk.portal.example|${A}|84532|5|${NOW + 3600}`);
    expect(m).toBe(createHmac('sha256', SECRET).update(payload).digest('base64url'));
  });

  it('round trips', () => {
    const token = issueReadToken(SECRET, CLAIMS);
    expect(verifyReadToken(SECRET, token, EXPECT)).toEqual(CLAIMS);
    expect(verifyReadToken(SECRET, token, { ...EXPECT, address: A.toUpperCase().replace('0X', '0x') })).toEqual(CLAIMS);
  });

  it('refuses a tampered payload', () => {
    const [, mac] = issueReadToken(SECRET, CLAIMS).split('.');
    const later = b64u(`v1|uk|uk.portal.example|${A}|84532|5|${NOW + 999_999}`);
    expect(verifyReadToken(SECRET, `${later}.${mac}`, EXPECT)).toBeNull();
    const otherEpoch = b64u(`v1|uk|uk.portal.example|${A}|84532|6|${NOW + 3600}`);
    expect(verifyReadToken(SECRET, `${otherEpoch}.${mac}`, EXPECT)).toBeNull();
  });

  it('refuses a tampered MAC, a truncated MAC and a MAC under another secret', () => {
    const token = issueReadToken(SECRET, CLAIMS);
    const [p, m] = token.split('.') as [string, string];
    const flipped = m.slice(0, -2) + (m.endsWith('AA') ? 'BB' : 'AA');
    expect(verifyReadToken(SECRET, `${p}.${flipped}`, EXPECT)).toBeNull();
    expect(verifyReadToken(SECRET, `${p}.${m.slice(0, 10)}`, EXPECT)).toBeNull();
    expect(verifyReadToken(SECRET, `${p}.`, EXPECT)).toBeNull();
    expect(verifyReadToken(SECRET, p, EXPECT)).toBeNull();
    expect(verifyReadToken(OTHER_SECRET, token, EXPECT)).toBeNull();
  });

  it('refuses an expired token, at the second of expiry', () => {
    const token = issueReadToken(SECRET, CLAIMS);
    expect(verifyReadToken(SECRET, token, { ...EXPECT, nowSeconds: NOW + 3599 })).not.toBeNull();
    expect(verifyReadToken(SECRET, token, { ...EXPECT, nowSeconds: NOW + 3600 })).toBeNull();
    expect(verifyReadToken(SECRET, token, { ...EXPECT, nowSeconds: NOW + 99_999 })).toBeNull();
  });

  it('refuses a token for another address', () => {
    expect(verifyReadToken(SECRET, issueReadToken(SECRET, CLAIMS), { ...EXPECT, address: B })).toBeNull();
  });

  it('refuses a token minted for another region (R5 v0.2)', () => {
    const token = issueReadToken(SECRET, CLAIMS);
    expect(verifyReadToken(SECRET, token, { ...EXPECT, region: 'eu' })).toBeNull();
    const fromEu = issueReadToken(SECRET, { ...CLAIMS, region: 'eu' });
    expect(verifyReadToken(SECRET, fromEu, EXPECT)).toBeNull();
    expect(verifyReadToken(SECRET, fromEu, { ...EXPECT, region: 'eu' })).not.toBeNull();
  });

  it('refuses a token minted for another portal host (R5 v0.2)', () => {
    const token = issueReadToken(SECRET, CLAIMS);
    expect(verifyReadToken(SECRET, token, { ...EXPECT, portal: 'eu.portal.example' })).toBeNull();
    const fromOther = issueReadToken(SECRET, { ...CLAIMS, portal: 'eu.portal.example' });
    expect(verifyReadToken(SECRET, fromOther, EXPECT)).toBeNull();
  });

  it('refuses the v0.1 payload without region and host, even correctly MACed', () => {
    expect(verifyReadToken(SECRET, forge(`v1|${'0x1234567890abcdef1234567890abcdef12345678'}|84532|5|${NOW + 3600}`), EXPECT)).toBeNull();
  });

  it('refuses to mint with a region or host that could forge a field', () => {
    expect(() => issueReadToken(SECRET, { ...CLAIMS, region: 'uk|eu' })).toThrow();
    expect(() => issueReadToken(SECRET, { ...CLAIMS, portal: 'a|b' })).toThrow();
    expect(() => issueReadToken(SECRET, { ...CLAIMS, region: '' })).toThrow();
    expect(() => issueReadToken(SECRET, { ...CLAIMS, portal: 'a b' })).toThrow();
  });

  it('refuses a token for another chain', () => {
    expect(verifyReadToken(SECRET, issueReadToken(SECRET, CLAIMS), { ...EXPECT, chainId: 8453 })).toBeNull();
  });

  it('refuses an old epoch', () => {
    const claims = verifyReadToken(SECRET, issueReadToken(SECRET, CLAIMS), EXPECT)!;
    expect(epochIsCurrent(claims, 5)).toBe(true);
    expect(epochIsCurrent(claims, 6)).toBe(false);
    expect(epochIsCurrent(claims, 4)).toBe(false);
  });

  it('refuses correctly MACed payloads of the wrong shape', () => {
    for (const payload of [
      `v2|uk|uk.portal.example|${A}|84532|5|${NOW + 3600}`,
      `v1|uk|uk.portal.example|${A}|84532|5`,
      `v1|uk|uk.portal.example|${A}|84532|5|${NOW + 3600}|extra`,
      `v1|uk|uk.portal.example|${A.toUpperCase()}|84532|5|${NOW + 3600}`,
      `v1|uk|uk.portal.example|${A}|084532|5|${NOW + 3600}`,
      `v1|uk|uk.portal.example|${A}|84532|-5|${NOW + 3600}`,
      `v1|uk|uk.portal.example|${A}|84532|5|1e12`,
      '',
    ]) {
      expect(verifyReadToken(SECRET, forge(payload), EXPECT)).toBeNull();
    }
  });

  it('refuses junk without throwing', () => {
    for (const junk of ['', '.', 'a.b.c', 'not a token', '!!!.???', 12, null, undefined, {}, 'x'.repeat(5000)]) {
      expect(verifyReadToken(SECRET, junk, EXPECT)).toBeNull();
    }
  });

  it('refuses to issue with a short secret or bad claims', () => {
    expect(() => issueReadToken(Buffer.alloc(31, 1), CLAIMS)).toThrow();
    expect(() => issueReadToken(SECRET, { ...CLAIMS, address: A.toUpperCase() })).toThrow();
    expect(() => issueReadToken(SECRET, { ...CLAIMS, epoch: -1 })).toThrow();
    expect(() => issueReadToken(SECRET, { ...CLAIMS, expiresAtSeconds: 1.5 })).toThrow();
  });
});
