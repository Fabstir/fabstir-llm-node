// FC2.1 — the security core of self-serve credential minting: prove that a
// signature over the server's challenge was produced by the claimed
// smart-account address, ON-CHAIN.
//
// Why viem, and ONLY here: a pure fiat user's Coinbase Smart Wallet is never
// deployed (the vault is the depositor, so the user never sends a transaction).
// An undeployed smart account signs with an ERC-6492-wrapped ERC-1271 signature
// — not an EOA `ecrecover` signature — which ethers v6 cannot verify. viem's
// verifyMessage / verifyTypedData do EOA + EIP-1271 + ERC-6492 in a single
// deployless eth_call. This module is the ONE place viem is imported (enforced
// by a grep gate); it reads a public RPC only — no key, no signing.
//
// Method-agnostic by design (IMPLEMENTATION R4): the real provider may sign a
// string (`personal_sign` → verifyMessage) or typed data (`eth_signTypedData_v4`
// → verifyTypedData); the R1 spike confirms which is live. `verifyAddressSignature`
// handles BOTH, dispatched by the proof shape, so the build never waits on it.
//
// The one thing viem does NOT do that security demands: viem collapses an RPC
// failure into `false` (indistinguishable from "invalid signature"). For an
// ownership check that is dangerous — a transient outage would read as "not the
// owner". So we wrap the transport to detect a genuine RPC failure and THROW
// (SignatureCheckUnavailableError) rather than return a misleading `false`. We
// still never return `true` unless the chain actually confirmed the signature.
import { createPublicClient, getAddress, http, type Transport, type TypedDataDomain } from 'viem';
// The (client, params) ACTION forms — the root `viem` export of `verifyMessage`
// is the single-arg offline util, which cannot do the on-chain 6492 call.
import { verifyMessage, verifyTypedData } from 'viem/actions';
import { baseSepolia } from 'viem/chains';
import { rpcUrl } from './balance';

/** Thrown when the RPC could not be reached / the verification call failed at
 *  the transport level — the ownership question is UNANSWERED, not answered
 *  "no". Callers must treat this as a 5xx (try again), never as "not the owner". */
export class SignatureCheckUnavailableError extends Error {
  constructor(cause?: unknown) {
    super('signature verification unavailable: the RPC call failed');
    this.name = 'SignatureCheckUnavailableError';
    (this as { cause?: unknown }).cause = cause;
  }
}

export interface MessageOwnershipProof {
  address: string;
  /** The exact string the wallet signed via `personal_sign`. */
  message: string;
  signature: string;
}

export interface TypedDataOwnershipProof {
  address: string;
  /** The EIP-712 payload the wallet signed via `eth_signTypedData_v4`. */
  typedData: {
    domain: TypedDataDomain;
    types: Record<string, ReadonlyArray<{ name: string; type: string }>>;
    primaryType: string;
    message: Record<string, unknown>;
  };
  signature: string;
}

export type OwnershipProof = MessageOwnershipProof | TypedDataOwnershipProof;

export interface VerifyOptions {
  /** Tests inject a viem `custom()` transport; production uses the public RPC. */
  transport?: Transport;
}

const ADDRESS_RE = /^0x[0-9a-fA-F]{40}$/;
const HEX_RE = /^0x[0-9a-fA-F]+$/;

function assertInputs(address: unknown, signature: unknown): asserts address is string {
  if (typeof address !== 'string' || !ADDRESS_RE.test(address)) {
    throw new Error(`address must be a 20-byte hex address, got "${String(address)}"`);
  }
  if (
    typeof signature !== 'string' ||
    !HEX_RE.test(signature) ||
    signature.length % 2 !== 0 || // whole bytes only
    signature.length < 6 // 0x + at least 2 bytes
  ) {
    throw new Error('signature must be non-empty even-length hex');
  }
}

/** Wrap a transport so a request that throws (RPC/connection failure) is
 *  recorded. viem would otherwise turn that into `false`; we turn it into a
 *  throw. A SUCCESSFUL call that returns 0x00 (invalid signature) does NOT throw
 *  and so is NOT flagged — that stays a clean `false`. */
function failureDetectingTransport(inner: Transport, onFailure: (cause: unknown) => void): Transport {
  return (params) => {
    const t = inner(params);
    return {
      ...t,
      request: (async (args: Parameters<typeof t.request>[0], reqOpts?: Parameters<typeof t.request>[1]) => {
        try {
          return await t.request(args, reqOpts);
        } catch (cause) {
          onFailure(cause);
          throw cause;
        }
      }) as typeof t.request,
    };
  };
}

/**
 * Verify that `signature` over the given message OR typed data was produced by
 * `address` (EOA, EIP-1271, or undeployed ERC-6492 smart account).
 *
 * Returns `true`/`false` for a clean on-chain answer; THROWS
 * SignatureCheckUnavailableError if the RPC could not be reached. Throws a plain
 * Error on malformed inputs (before any RPC call).
 */
export async function verifyAddressSignature(proof: OwnershipProof, opts: VerifyOptions = {}): Promise<boolean> {
  assertInputs(proof.address, proof.signature);

  let transportFailure: unknown;
  const inner = opts.transport ?? http(rpcUrl());
  const client = createPublicClient({
    chain: baseSepolia,
    transport: failureDetectingTransport(inner, (cause) => {
      transportFailure = cause;
    }),
  });

  const signature = proof.signature as `0x${string}`;
  // viem's ABI encoder rejects a non-checksum address (isAddress strict); the
  // on-chain check is case-insensitive, so normalise any casing to checksum.
  const address = getAddress(proof.address.toLowerCase());

  let verified: boolean;
  if ('typedData' in proof) {
    // viem's verifyTypedData is generic over `types`; our payload is
    // runtime-shaped (the server's own challenge), so we cast the whole params
    // object once. The on-chain call is what actually validates it.
    const params = {
      address,
      domain: proof.typedData.domain,
      types: proof.typedData.types,
      primaryType: proof.typedData.primaryType,
      message: proof.typedData.message,
      signature,
    } as unknown as Parameters<typeof verifyTypedData>[1];
    verified = await verifyTypedData(client, params);
  } else {
    verified = await verifyMessage(client, {
      address,
      message: proof.message,
      signature,
    });
  }

  // If the transport threw at any point, viem will have collapsed it to
  // `verified === false`. Surface it as an outage instead of a false negative.
  if (transportFailure !== undefined) throw new SignatureCheckUnavailableError(transportFailure);
  return verified;
}

export type SignatureVerifier = (proof: OwnershipProof, opts?: VerifyOptions) => Promise<boolean>;

let verifierOverride: SignatureVerifier | undefined;

/** Route tests inject a fake verifier so they need no viem/RPC; undefined
 *  restores the real on-chain check. */
export function setSignatureVerifierForTest(fn: SignatureVerifier | undefined): void {
  verifierOverride = fn;
}

export function getSignatureVerifier(): SignatureVerifier {
  return verifierOverride ?? verifyAddressSignature;
}
