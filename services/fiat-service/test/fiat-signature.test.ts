// FC2.1 — on-chain signature verification (the security core of self-serve
// minting). A pure fiat user's smart account is NEVER deployed (the vault is the
// depositor), so its `personal_sign` produces an ERC-6492-wrapped ERC-1271
// signature, not an EOA `ecrecover` one. viem's verifyMessage/verifyTypedData do
// EOA + EIP-1271 + ERC-6492 in ONE deployless eth_call.
//
// These tests exercise the REAL viem code path end-to-end and stub only the
// TRANSPORT (a viem `custom()` transport whose eth_call returns the ABI-encoded
// bool the universal validator would return): 0x…01 = valid, 0x…00 = invalid.
// The one thing viem does NOT give us for free — surfacing an RPC outage instead
// of silently reporting "invalid" — is what `fiat-signature.ts` adds and what
// the transport-error test pins.
import { describe, expect, it } from 'vitest';
import { custom, hashMessage, type Transport } from 'viem';
import {
  SignatureCheckUnavailableError,
  verifyAddressSignature,
  type TypedDataOwnershipProof,
} from '../src/lib/fiat-signature';

const ADDR = '0x1234567890abcDEF1234567890abcdef12345678';
const ADDR_LC = ADDR.toLowerCase();
// A plausibly-shaped ERC-6492-wrapped signature (long hex, ends in the 6492
// magic). Its BYTES are irrelevant here — the stubbed validator decides
// valid/invalid; only the hex SHAPE has to pass input validation.
const SIG = '0x' + 'ab'.repeat(210) + '6492'.repeat(8);
// The 32-byte ABI-encoded booleans the ERC-6492 validator returns.
const VALID = '0x0000000000000000000000000000000000000000000000000000000000000001';
const INVALID = '0x0000000000000000000000000000000000000000000000000000000000000000';

type RpcCall = { method: string; params: unknown };

/** A viem transport whose eth_call is driven by `handler`. retryCount 0 keeps a
 *  thrown error from being retried, so the call log is deterministic. */
function stub(handler: (method: string, params: unknown) => Promise<unknown>): {
  transport: Transport;
  calls: RpcCall[];
} {
  const calls: RpcCall[] = [];
  const transport = custom(
    {
      request: ({ method, params }: { method: string; params?: unknown }) => {
        calls.push({ method, params });
        return handler(method, params);
      },
    },
    { retryCount: 0 }
  );
  return { transport, calls };
}

const alwaysValid = () => stub(async (m) => (m === 'eth_call' ? VALID : Promise.reject(new Error(`unexpected ${m}`))));
const alwaysInvalid = () => stub(async (m) => (m === 'eth_call' ? INVALID : Promise.reject(new Error(`unexpected ${m}`))));

const TYPED: TypedDataOwnershipProof['typedData'] = {
  domain: { name: 'Platformless AI', version: '1', chainId: 84532 },
  types: {
    Ownership: [
      { name: 'wallet', type: 'address' },
      { name: 'nonce', type: 'string' },
    ],
  },
  primaryType: 'Ownership',
  message: { wallet: ADDR_LC, nonce: 'abc123' },
};

describe('verifyAddressSignature — message (personal_sign) path', () => {
  it('returns true when the on-chain validator confirms, binding the address + message hash into the call', async () => {
    const { transport, calls } = alwaysValid();
    const ok = await verifyAddressSignature({ address: ADDR, message: 'hello world', signature: SIG }, { transport });
    expect(ok).toBe(true);

    const ethCall = calls.find((c) => c.method === 'eth_call');
    expect(ethCall).toBeDefined();
    // The deployless verifier is called with (address, messageHash, signature)
    // ABI-encoded into the constructor data — so the exact address and the exact
    // message hash the server chose MUST appear in the calldata.
    const data = String((ethCall!.params as [{ data: string }])[0].data).toLowerCase();
    expect(data).toContain(ADDR_LC.slice(2));
    expect(data).toContain(hashMessage('hello world').slice(2));
  });

  it('returns false for a wrong-signer signature (validator says invalid)', async () => {
    const { transport } = alwaysInvalid();
    expect(await verifyAddressSignature({ address: ADDR, message: 'hello world', signature: SIG }, { transport })).toBe(false);
  });

  it('returns false for a tampered message (validator says invalid)', async () => {
    const { transport } = alwaysInvalid();
    expect(await verifyAddressSignature({ address: ADDR, message: 'TAMPERED', signature: SIG }, { transport })).toBe(false);
  });

  it('THROWS (never silently passes) when the RPC transport fails', async () => {
    const { transport } = stub(async (m) => {
      if (m === 'eth_call') throw new Error('ECONNREFUSED sepolia.base.org');
      throw new Error(`unexpected ${m}`);
    });
    await expect(
      verifyAddressSignature({ address: ADDR, message: 'hello world', signature: SIG }, { transport })
    ).rejects.toBeInstanceOf(SignatureCheckUnavailableError);
  });
});

describe('verifyAddressSignature — typed-data (eth_signTypedData_v4) path', () => {
  it('returns true when the validator confirms a typed-data proof', async () => {
    const { transport, calls } = alwaysValid();
    const ok = await verifyAddressSignature({ address: ADDR, typedData: TYPED, signature: SIG }, { transport });
    expect(ok).toBe(true);
    expect(calls.some((c) => c.method === 'eth_call')).toBe(true);
  });

  it('returns false when the validator rejects a typed-data proof', async () => {
    const { transport } = alwaysInvalid();
    expect(await verifyAddressSignature({ address: ADDR, typedData: TYPED, signature: SIG }, { transport })).toBe(false);
  });
});

describe('verifyAddressSignature — input validation (before any RPC)', () => {
  it('rejects a malformed address and a non-hex signature without touching the transport', async () => {
    const { transport, calls } = alwaysValid();
    await expect(
      verifyAddressSignature({ address: '0xnope', message: 'm', signature: SIG }, { transport })
    ).rejects.toThrow();
    await expect(
      verifyAddressSignature({ address: ADDR, message: 'm', signature: 'not-hex' }, { transport })
    ).rejects.toThrow();
    await expect(
      verifyAddressSignature({ address: ADDR, message: 'm', signature: '0xabc' }, { transport }) // odd length
    ).rejects.toThrow();
    expect(calls.length).toBe(0);
  });

  it('an input error is NOT reported as a transport outage', async () => {
    const { transport } = alwaysValid();
    await expect(
      verifyAddressSignature({ address: '0xnope', message: 'm', signature: SIG }, { transport })
    ).rejects.not.toBeInstanceOf(SignatureCheckUnavailableError);
  });
});
