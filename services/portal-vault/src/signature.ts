// R1: prove that a signature over the server's challenge was produced by the
// claimed address, ON-CHAIN. Ported from the fiat service's fiat-signature.ts.
//
// Why viem, and ONLY here: signatures come from plain keys and from smart
// accounts. A smart account signs through its contract (ERC-1271) and, when it
// is not yet deployed, in the ERC-6492 envelope. viem's verifyMessage and
// verifyTypedData actions do plain keys, ERC-1271 and ERC-6492 in a single
// deployless eth_call. This module is the ONE place viem is imported (a test
// greps the source tree to enforce it); it reads a public RPC only: no key, no
// signing.
//
// The one thing viem does NOT do that security demands: viem collapses an RPC
// failure into `false`, indistinguishable from "invalid signature". For an
// ownership check that is wrong: a passing outage would read as "not the
// owner". So the transport is wrapped to detect a genuine RPC failure and the
// check THROWS SignatureCheckUnavailableError instead of returning a misleading
// `false`. It still never returns `true` unless the chain confirmed.
//
// Differences from the fiat service, both from the specification:
//   - the chain is chosen from the configured chain id (84532 or 8453);
//   - two or more RPC URLs may be configured and are tried in order. The next
//     is asked only when the one before FAILED; a clean answer, valid or
//     invalid, is final.
//
// v0.2 (R1):
//   - an `execution reverted` answer to the verification call is the chain's
//     verdict, not an outage, and gives "invalid". Found live: a smart-account
//     signature offered for another address makes the validator revert. Only
//     an error object returned by the node in a successful HTTP answer counts;
//     an HTTP-level failure, a timeout or a network error is "unavailable"
//     whatever its body says (round 2);
//   - a signature longer than 8 KiB is refused before any chain call;
//   - `withConcurrencyLimit` bounds the checks in flight.
import { RpcRequestError, createPublicClient, getAddress, http, type Chain, type Transport, type TypedDataDomain } from 'viem';
// The (client, params) ACTION forms. The root `viem` export of `verifyMessage`
// is the single-argument offline utility, which cannot do the on-chain call.
import { verifyMessage, verifyTypedData } from 'viem/actions';
import { base, baseSepolia } from 'viem/chains';

/** Thrown when no RPC could be asked: the ownership question is UNANSWERED,
 *  not answered "no". Callers answer 503, never "invalid". */
export class SignatureCheckUnavailableError extends Error {
  constructor(cause?: unknown) {
    super('signature verification unavailable: the RPC call failed');
    this.name = 'SignatureCheckUnavailableError';
    (this as { cause?: unknown }).cause = cause;
  }
}

export interface MessageOwnershipProof {
  address: string;
  /** The exact string the wallet signed with `personal_sign`. */
  message: string;
  signature: string;
}

export interface TypedDataOwnershipProof {
  address: string;
  /** The EIP-712 payload the wallet signed with `eth_signTypedData_v4`. */
  typedData: {
    domain: { name?: string; version?: string; chainId?: number };
    types: Record<string, ReadonlyArray<{ name: string; type: string }>>;
    primaryType: string;
    message: Record<string, unknown>;
  };
  signature: string;
}

export type OwnershipProof = MessageOwnershipProof | TypedDataOwnershipProof;

/** True or false for a clean on-chain answer; throws
 *  SignatureCheckUnavailableError when the chain could not be asked; throws a
 *  plain Error on malformed input, before any RPC call. */
export type SignatureVerifier = (proof: OwnershipProof) => Promise<boolean>;

const CHAINS: ReadonlyMap<number, Chain> = new Map<number, Chain>([
  [baseSepolia.id, baseSepolia],
  [base.id, base],
]);

export const SUPPORTED_CHAIN_IDS: ReadonlySet<number> = new Set(CHAINS.keys());

const ADDRESS_RE = /^0x[0-9a-fA-F]{40}$/;
const HEX_RE = /^0x[0-9a-fA-F]+$/;
/** R1 v0.2: 8 KiB of signature, as hex characters after the 0x. An ERC-6492
 *  envelope carries deployment data and is long, but nowhere near this. */
export const MAX_SIGNATURE_HEX_CHARS = 16_384;

export function isWellFormedSignature(signature: unknown): signature is string {
  return (
    typeof signature === 'string' &&
    signature.length <= MAX_SIGNATURE_HEX_CHARS + 2 &&
    HEX_RE.test(signature) &&
    signature.length % 2 === 0 && // whole bytes only
    signature.length >= 6 // 0x + at least 2 bytes
  );
}

function assertInputs(address: unknown, signature: unknown): void {
  // Neither message echoes the value: a signature must never reach a log.
  if (typeof address !== 'string' || !ADDRESS_RE.test(address)) {
    throw new Error('address must be a 20-byte hex address');
  }
  if (!isWellFormedSignature(signature)) {
    throw new Error('signature must be non-empty even-length hex of at most 8 KiB');
  }
}

const REVERT_PHRASE_RE = /execution reverted/i;

/** What one verification attempt learned about the HTTP answer behind it.
 *  viem raises a JSON-RPC error object the same way whether it arrived in a
 *  200 answer or in the body of a 502, so the status is noted here, per
 *  attempt, by the http transport's response hook. */
interface HttpNote {
  /** True for the service's own http transports; false for an injected one. */
  overHttp: boolean;
  /** Status of the last HTTP answer of this attempt, if there was one. */
  status: number | undefined;
}

/** The JSON-RPC error RESPONSE inside a request error, if that is what it is:
 *  the node's error object, which the http transport raises as
 *  `RpcRequestError` and viem then wraps once in a typed error. */
function rpcErrorResponse(error: unknown): RpcRequestError | null {
  if (error instanceof RpcRequestError) return error;
  const cause = (error as { cause?: unknown } | null)?.cause;
  return cause instanceof RpcRequestError ? cause : null;
}

/** R1 v0.2: is this request error the NODE saying the call reverted?
 *
 *  Only a JSON-RPC error RESPONSE counts: the request succeeded at HTTP level
 *  (a 2xx answer) and the node returned an error object with code 3, or with
 *  the phrase "execution reverted" in its own message. That is the chain's
 *  verdict.
 *
 *  Nothing else is a verdict, whatever text it carries: an HTTP-level failure
 *  (a 502 or a 429 from a gateway, even when its body is, or quotes, such an
 *  error object), a timeout, a network error, or any other error. Those mean
 *  the chain was NOT asked, and stay "unavailable". */
export function isExecutionReverted(error: unknown, note?: { overHttp: boolean; status: number | undefined }): boolean {
  const rpc = rpcErrorResponse(error);
  if (!rpc) return false;
  if (note && note.overHttp && !(note.status !== undefined && note.status >= 200 && note.status < 300)) return false;
  if (rpc.code === 3) return true;
  // `details` is the message of the node's error object, and nothing else.
  return typeof rpc.details === 'string' && REVERT_PHRASE_RE.test(rpc.details);
}

/** Wrap a transport so a request that throws (RPC or connection failure) is
 *  recorded. viem would otherwise turn that into `false`. A SUCCESSFUL call
 *  that returns 0x00 (invalid signature) does not throw and is not flagged:
 *  that stays a clean `false`. Nor is a call the node reports as reverted
 *  (v0.2): the chain was asked and it answered; viem turns that into `false`
 *  and here it stays `false`. */
function failureDetectingTransport(inner: Transport, note: HttpNote, onFailure: (cause: unknown) => void): Transport {
  return (params) => {
    const t = inner(params);
    return {
      ...t,
      request: (async (args: Parameters<typeof t.request>[0], reqOpts?: Parameters<typeof t.request>[1]) => {
        try {
          return await t.request(args, reqOpts);
        } catch (cause) {
          if (!isExecutionReverted(cause, note)) onFailure(cause);
          throw cause;
        }
      }) as typeof t.request,
    };
  };
}

export interface SignatureVerifierOptions {
  /** The one configured chain: 84532 (Base Sepolia) or 8453 (Base). */
  chainId: number;
  /** RPC URLs, tried in order. */
  rpcUrls?: readonly string[];
  /** Tests inject viem `custom()` transports in place of the URLs. */
  transports?: readonly Transport[];
  /** Per-request timeout for the http transports, in milliseconds. */
  timeoutMs?: number;
}

export function createSignatureVerifier(opts: SignatureVerifierOptions): SignatureVerifier {
  const chain = CHAINS.get(opts.chainId);
  if (!chain) throw new Error(`unsupported chain id ${String(opts.chainId)}: only 84532 and 8453 are known`);

  // One transport per attempt, so each attempt has its own note of the HTTP
  // status it was answered with and attempts running together never share one.
  type TransportFactory = (note: HttpNote) => Transport;
  const factories: readonly TransportFactory[] = opts.transports
    ? opts.transports.map((transport) => (note: HttpNote) => {
        note.overHttp = false;
        return transport;
      })
    : (opts.rpcUrls ?? []).map((url) => (note: HttpNote) => {
        note.overHttp = true;
        return http(url, {
          timeout: opts.timeoutMs ?? 8_000,
          retryCount: 1,
          onFetchResponse: (response) => {
            note.status = response.status;
          },
        });
      });
  if (factories.length === 0) throw new Error('at least one RPC URL is required');

  async function askOne(factory: TransportFactory, proof: OwnershipProof): Promise<{ answered: true; verified: boolean } | { answered: false; cause: unknown }> {
    let failed = false;
    let failure: unknown;
    const note: HttpNote = { overHttp: true, status: undefined };
    const client = createPublicClient({
      chain,
      transport: failureDetectingTransport(factory(note), note, (cause) => {
        failed = true;
        failure = cause;
      }),
    });

    const signature = proof.signature as `0x${string}`;
    // viem's ABI encoder rejects a non-checksum address; the on-chain check is
    // case-insensitive, so normalise any casing to checksum.
    const address = getAddress(proof.address.toLowerCase());

    let verified: boolean;
    try {
      if ('typedData' in proof) {
        // verifyTypedData is generic over `types`; this payload is
        // runtime-shaped (the server's own challenge), so the params object is
        // cast once. The on-chain call is what validates it.
        const params = {
          address,
          domain: proof.typedData.domain as TypedDataDomain,
          types: proof.typedData.types,
          primaryType: proof.typedData.primaryType,
          message: proof.typedData.message,
          signature,
        } as unknown as Parameters<typeof verifyTypedData>[1];
        verified = await verifyTypedData(client, params);
      } else {
        verified = await verifyMessage(client, { address, message: proof.message, signature });
      }
    } catch (cause) {
      // viem normally returns false rather than throwing. If it throws, the
      // question was not answered: never "invalid", never "valid".
      return { answered: false, cause };
    }
    // If the transport threw at any point, viem collapsed it to `false`.
    if (failed) return { answered: false, cause: failure };
    return { answered: true, verified };
  }

  return async function verifyAddressSignature(proof: OwnershipProof): Promise<boolean> {
    assertInputs(proof.address, proof.signature);
    let lastCause: unknown;
    for (const factory of factories) {
      const result = await askOne(factory, proof);
      if (result.answered) return result.verified;
      lastCause = result.cause;
    }
    throw new SignatureCheckUnavailableError(lastCause);
  };
}

/** R1 v0.2: at most `max` checks in flight. One beyond that is answered
 *  "unavailable" at once (503 with Retry-After; the nonce stays live), and the
 *  verifier is not called, so slow RPCs cannot pile requests up without bound. */
export function withConcurrencyLimit(verify: SignatureVerifier, max: number): SignatureVerifier {
  if (!Number.isSafeInteger(max) || max <= 0) throw new Error('verification limit must be a positive integer');
  let inFlight = 0;
  return async (proof) => {
    if (inFlight >= max) throw new SignatureCheckUnavailableError(new Error('too many verifications in flight'));
    inFlight += 1;
    try {
      return await verify(proof);
    } finally {
      inFlight -= 1;
    }
  };
}
