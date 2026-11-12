// A scripted StripeApi for tests: records every refund request (with its
// idempotency key and metadata) and lets a test script failures per call.
import { StripeApiError, type StripeApi, type StripeRefundRequest } from '../../src/lib/stripe';

export interface FakeStripeOptions {
  /** Decide the fate of each refund call (1-based call number). Return an
   *  Error to make it throw; `landed: true` makes the refund exist at Stripe
   *  even though the call threw (the timeout-after-success case). */
  onRefund?: (request: StripeRefundRequest, call: number) => { error: Error; landed?: boolean } | undefined;
  /** Make the look-up after an ambiguous error throw. */
  lookupError?: Error;
  cardCountry?: string;
  /** What `paymentState` answers per PaymentIntent (default: nothing known,
   *  i.e. not a payment of this product). An Error makes the read throw. */
  paymentState?: (paymentIntentId: string) => import('../../src/lib/stripe').StripePaymentState | Error;
}

export function fakeStripeApi(opts: FakeStripeOptions = {}) {
  /** Every createRefund call, in order, whether or not it succeeded. */
  const calls: StripeRefundRequest[] = [];
  /** The refunds that exist "at Stripe". */
  const made: Array<StripeRefundRequest & { id: string }> = [];
  const lookups: Array<{ paymentIntentId: string; key: string }> = [];
  const countryLookups: Array<{ paymentIntentId: string; timeoutMs?: number }> = [];
  const stateReads: string[] = [];
  const api: StripeApi = {
    async createRefund(request) {
      calls.push(request);
      const fate = opts.onRefund?.(request, calls.length);
      if (fate) {
        if (fate.landed) made.push({ ...request, id: `re_${made.length + 1}` });
        throw fate.error;
      }
      const id = `re_${made.length + 1}`;
      made.push({ ...request, id });
      return { id, status: 'succeeded' };
    },
    async cardCountry(paymentIntentId, o) {
      countryLookups.push({ paymentIntentId, ...(o?.timeoutMs !== undefined ? { timeoutMs: o.timeoutMs } : {}) });
      return opts.cardCountry ?? 'GB';
    },
    async paymentState(paymentIntentId) {
      stateReads.push(paymentIntentId);
      const state = opts.paymentState?.(paymentIntentId) ?? {};
      if (state instanceof Error) throw state;
      return state;
    },
    async findRefund(paymentIntentId, key) {
      lookups.push({ paymentIntentId, key });
      if (opts.lookupError) throw opts.lookupError;
      const hit = made.find((r) => r.paymentIntentId === paymentIntentId && r.metadata.fc1Key === key);
      return hit ? { id: hit.id, status: 'succeeded' } : undefined;
    },
  };
  return { api, calls, made, lookups, countryLookups, stateReads };
}

/** Stripe answered and refused (a 4xx): nothing was done. */
export const stripeRefused = (message = 'card refund refused', code?: string) => new StripeApiError(message, 402, code);
/** No answer from Stripe: the refund may or may not have been made. */
export const stripeNoAnswer = (message = 'socket hang up') => new StripeApiError(message, undefined, undefined);
