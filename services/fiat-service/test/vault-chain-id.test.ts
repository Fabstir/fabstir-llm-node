// The vault's chain id setting. `.env.example` ships `FIAT_CHAIN_ID=` blank,
// and BigInt('') is 0n: a create signed for chain 0 is journalled in the
// outbox, never accepted by the chain, and ends in `vault_stuck`.
import { describe, expect, it } from 'vitest';
import { vaultChainId } from '../src/lib/fiat-vault';
import { DEFAULT_CHAIN_ID } from '../src/lib/vault-outbox';

describe('vaultChainId (FIAT_CHAIN_ID)', () => {
  it('unset, blank and whitespace all mean Base Sepolia', () => {
    // mutation: BigInt(env.FIAT_CHAIN_ID ?? default) → blank answers 0n.
    expect(DEFAULT_CHAIN_ID).toBe(84532n);
    expect(vaultChainId({})).toBe(84532n);
    expect(vaultChainId({ FIAT_CHAIN_ID: '' })).toBe(84532n);
    expect(vaultChainId({ FIAT_CHAIN_ID: '   ' })).toBe(84532n);
  });

  it('a positive integer is used as given', () => {
    expect(vaultChainId({ FIAT_CHAIN_ID: '8453' })).toBe(8453n);
    expect(vaultChainId({ FIAT_CHAIN_ID: ' 84532 ' })).toBe(84532n);
  });

  it.each(['0', '-1', '0x14a34', '84532.0', 'base', '84532 # sepolia', '08453'])('refuses %j at backend build', (bad) => {
    // mutation: drop the shape check → '0' signs for chain 0, '0x14a34' is silently accepted.
    expect(() => vaultChainId({ FIAT_CHAIN_ID: bad })).toThrow(/FIAT_CHAIN_ID must be a positive integer/);
  });
});
