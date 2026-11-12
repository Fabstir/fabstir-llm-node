// THE REFUND INVARIANT, walked exhaustively (see the comment above
// `reverseCashoutLeg` in src/lib/ledger.ts):
//
//     final balance  ==  credits purchased  -  the net share of the cash that
//                                              actually reached the card
//
// A small Stripe simulator holds the truth (which refunds exist, which failed,
// the charge's current `amount_refunded`) and records every event it would
// send, each `charge.refunded` carrying the cumulative figure AT THE MOMENT IT
// WAS EMITTED (so a snapshot delivered after a later failure is stale, exactly
// as in production). The table then walks EVERY ordering of the steps
//
//     plan leg · refund succeeds (seen by the service, or not: a crash) ·
//     Stripe refuses · refund fails at the bank · failure event delivered ·
//     charge.refunded delivered (fresh or stale) · sweep
//
// for one leg and for two legs on one charge, with the sweep at every
// position, and after delivering whatever is still undelivered asserts the
// invariant, the per-charge figures, and that no leg is left pending.
import { describe, expect, it } from 'vitest';
import { CreditsLedger, MemoryLedgerStore } from '../src/lib/ledger';
import { REPOST_WITHIN_MS, recoverPendingCashoutLegs, requestCashout, type CashoutDeps } from '../src/lib/fiat-cashout-service';
import { handleStripeEvent, type StripeWebhookDeps } from '../src/lib/stripe-webhook';
import { allowedBillingCountries } from '../src/lib/sales-countries';
import { StripeApiError, type StripeApi } from '../src/lib/stripe';
import { FiatCredentials } from '../src/lib/fiat-credentials';
import { chargeRefunded, refundUpdated } from './fixtures/stripe-events';

const USER = 'user-1';
const PI = 'pi_1';
const T0 = 1_794_000_000_000;
const DAY_MS = 86_400_000;
const credits = (n: number) => BigInt(n) * 10_000n;
/** 1,000 credits bought for 1,200 cents (20% VAT). */
const BOUGHT = 1000;
const GROSS = 1200;
/** Credits each leg asks for: 480 and 360 cents with the VAT. */
const LEG_NET = [400, 300] as const;
/** A refund made in the Stripe dashboard: 240 cents = 200 credits. */
const DASH_CENTS = 240;
const DASH_NET = 200;

type Emitted =
  | { kind: 'snapshot'; owner: string; cum: number; delivered: boolean }
  | { kind: 'failure'; owner: string; refundId: string; amount: number; key?: string; delivered: boolean };

interface SimRefund {
  id: string;
  amount: number;
  status: 'succeeded' | 'failed';
  key?: string;
  /** The service has been TOLD this refund failed: by the failure event, or by
   *  Stripe answering a request or a look-up with its failed status. */
  failureKnown?: boolean;
}

/** Stripe, as far as refunds on one charge go. */
class StripeSim {
  /** The charge's current amount_refunded: refunds made and not failed. */
  refundedNow = 0;
  refunds: SimRefund[] = [];
  events: Emitted[] = [];
  private n = 0;

  make(owner: string, amount: number, key?: string, status: 'succeeded' | 'failed' = 'succeeded'): SimRefund {
    const refund: SimRefund = { id: `re_${++this.n}`, amount, status, ...(key ? { key } : {}) };
    this.refunds.push(refund);
    if (status === 'succeeded') {
      this.refundedNow += amount;
      this.events.push({ kind: 'snapshot', owner, cum: this.refundedNow, delivered: false });
    } else {
      this.events.push({ kind: 'failure', owner, refundId: refund.id, amount, ...(key ? { key } : {}), delivered: false });
    }
    return refund;
  }

  /** The refund never reaches the card: Stripe's figure falls. */
  bankFails(owner: string, refund: SimRefund): void {
    if (refund.status === 'failed') return;
    refund.status = 'failed';
    this.refundedNow -= refund.amount;
    this.events.push({
      kind: 'failure',
      owner,
      refundId: refund.id,
      amount: refund.amount,
      ...(refund.key ? { key: refund.key } : {}),
      delivered: false,
    });
  }

  byKey(key: string): SimRefund | undefined {
    return this.refunds.find((r) => r.key === key);
  }

  /** The client the service, the sweep and the webhook talk to. */
  api(): StripeApi {
    return {
      createRefund: async (request) => {
        // Idempotent by key, as Stripe is inside 24 hours.
        const existing = this.byKey(request.idempotencyKey);
        if (existing) {
          if (existing.status === 'failed') existing.failureKnown = true;
          return { id: existing.id, status: existing.status };
        }
        const made = this.make(`leg:${request.idempotencyKey}`, request.amountCents!, request.idempotencyKey);
        return { id: made.id, status: made.status };
      },
      findRefund: async (_pi, key) => {
        const r = this.byKey(key);
        if (r?.status === 'failed') r.failureKnown = true;
        return r ? { id: r.id, status: r.status } : undefined;
      },
      paymentState: async () => ({ app: 'platformless-ai', amountRefundedCents: this.refundedNow }),
      cardCountry: async () => 'GB',
    };
  }
}

async function world() {
  const store = new MemoryLedgerStore();
  const clock = { nowMs: T0 };
  const ledger = await CreditsLedger.open(store, { now: () => clock.nowMs });
  await ledger.purchase(USER, credits(BOUGHT), 'evt_buy', { paymentIntentId: PI, grossCents: GROSS, taxCents: GROSS - BOUGHT });
  const sim = new StripeSim();
  const webhook: StripeWebhookDeps = {
    ledger,
    stripe: sim.api(),
    onAlarm: () => {},
    allowedCountries: allowedBillingCountries({}),
    log: () => {},
    now: () => clock.nowMs,
  };
  let eventSeq = 0;
  const deliver = async (e: Emitted) => {
    e.delivered = true;
    if (e.kind === 'failure') {
      const refund = sim.refunds.find((r) => r.id === e.refundId);
      if (refund) refund.failureKnown = true;
    }
    const res =
      e.kind === 'snapshot'
        ? await handleStripeEvent(webhook, chargeRefunded({ eventId: `evt_s_${++eventSeq}`, paymentIntent: PI, amountCents: GROSS, amountRefundedCents: e.cum }))
        : await handleStripeEvent(
            webhook,
            refundUpdated({
              eventId: `evt_f_${++eventSeq}`,
              refundId: e.refundId,
              paymentIntent: PI,
              amountCents: e.amount,
              status: 'failed',
              metadata: e.key ? { fc1Origin: 'cashout', fc1Key: e.key } : {},
            })
          );
    if (res.status !== 200) throw new Error(`event answered ${res.status}`);
  };
  return { store, clock, ledger, sim, deliver };
}

/** One step of one actor. Actors: 'L0', 'L1' (refund legs) and 'D' (a refund
 *  made in the dashboard). */
type Step =
  | 'plan' // the debit is journaled (planCashout)
  | 'succeeds' // Stripe makes the refund and the service hears: confirm
  | 'lands' // Stripe makes the refund; the service never hears (a crash)
  | 'refused' // Stripe refuses: the service reverses
  | 'failsAtOnce' // Stripe answers with a refund already `failed`
  | 'snapshot' // this refund's charge.refunded is delivered (stale if a failure came since)
  | 'bankFails' // the refund fails at the bank (nothing is delivered yet)
  | 'failureEvent' // charge.refund.updated (failed) is delivered
  | 'dashRefund'; // an operator refunds in the dashboard

interface Move {
  actor: string;
  step: Step | 'sweep';
}

/** Every life a refund leg can have, as an ordered chain of steps. */
const LEG_CHAINS: Step[][] = [
  ['plan', 'succeeds', 'snapshot'],
  ['plan', 'succeeds', 'snapshot', 'bankFails', 'failureEvent'],
  ['plan', 'succeeds', 'bankFails', 'snapshot', 'failureEvent'], // stale snapshot, before the failure is known
  ['plan', 'succeeds', 'bankFails', 'failureEvent', 'snapshot'], // stale snapshot, after the failure is known
  ['plan', 'lands', 'snapshot'],
  ['plan', 'lands', 'snapshot', 'bankFails', 'failureEvent'],
  ['plan', 'lands', 'bankFails', 'snapshot', 'failureEvent'],
  ['plan', 'lands', 'bankFails', 'failureEvent', 'snapshot'],
  ['plan', 'refused'],
  ['plan'], // the process died before Stripe was called
  ['plan', 'failsAtOnce', 'failureEvent'],
];

const DASH_CHAINS: Step[][] = [
  ['dashRefund', 'snapshot'],
  ['dashRefund', 'snapshot', 'bankFails', 'failureEvent'],
  ['dashRefund', 'bankFails', 'snapshot', 'failureEvent'],
  ['dashRefund', 'bankFails', 'failureEvent', 'snapshot'],
];

/** Every interleaving of two chains that keeps each chain's own order. */
function interleave(a: Move[], b: Move[]): Move[][] {
  if (a.length === 0) return [b];
  if (b.length === 0) return [a];
  return [
    ...interleave(a.slice(1), b).map((rest) => [a[0]!, ...rest]),
    ...interleave(a, b.slice(1)).map((rest) => [b[0]!, ...rest]),
  ];
}

/** The ordering as it is, and with one sweep after each of its steps. */
function withSweeps(moves: Move[]): Move[][] {
  const out: Move[][] = [moves];
  for (let i = 1; i <= moves.length; i++) {
    out.push([...moves.slice(0, i), { actor: '*', step: 'sweep' }, ...moves.slice(i)]);
  }
  return out;
}

const chain = (actor: string, steps: Step[]): Move[] => steps.map((step) => ({ actor, step }));
const describeMoves = (moves: Move[]) => moves.map((m) => (m.step === 'sweep' ? 'SWEEP' : `${m.actor}.${m.step}`)).join(' > ');

/** Run one ordering and return what is wrong with the end state, or null. */
async function walk(moves: Move[], sweepAgeMs: number): Promise<string | null> {
  const w = await world();
  const legs = new Map<string, { key: string; net: number; gross: number }>();
  let dash: SimRefund | undefined;
  const sweep = async (ageMs: number) => {
    await recoverPendingCashoutLegs({ ledger: w.ledger, stripe: w.sim.api(), onAlarm: () => {}, now: () => T0 + ageMs, graceMs: 0 });
  };
  const refundOf = (actor: string): SimRefund => {
    const r = actor === 'D' ? dash : w.sim.byKey(legs.get(actor)!.key);
    if (!r) throw new Error(`no refund for ${actor}`);
    return r;
  };
  /** The actor's undelivered event of this kind, if this world has one (a
   *  sweep that came first can have settled the leg another way). */
  const eventOf = (actor: string, kind: Emitted['kind']): Emitted | undefined => {
    const owner = actor === 'D' ? 'D' : `leg:${legs.get(actor)!.key}`;
    return w.sim.events.find((x) => x.owner === owner && x.kind === kind && !x.delivered);
  };

  // SAFETY, checked after EVERY step, not only at the end: the ledger never
  // holds a debit for a refund "made outside this service" that is not backed
  // by cash that really went out. What it has debited beyond its own live legs
  // must be covered by refunds that reached the card (and are not one of those
  // live legs), plus refunds that have failed without the service having been
  // told yet. A phantom debit breaks this the moment it is journaled, even if
  // a later event would happen to heal it.
  const safety = (after: string): string | null => {
    const charge = w.ledger.charge(PI)!;
    const live = w.ledger.allCashoutLegs().filter((l) => l.state !== 'reversed');
    const liveKeys = new Set(live.map((l) => l.key));
    const liveGross = live.reduce((sum, l) => sum + l.grossCents, 0n);
    const outsideDebited = charge.refundedGrossCents - liveGross;
    let backing = 0;
    for (const r of w.sim.refunds) {
      if (r.status === 'succeeded' && !(r.key && liveKeys.has(r.key))) backing += r.amount;
      if (r.status === 'failed' && !r.failureKnown) backing += r.amount;
    }
    if (outsideDebited > BigInt(backing)) {
      return `after ${after}: ${outsideDebited} cents debited as refunded outside this service, only ${backing} backed by cash`;
    }
    // Nor does a leg stay debited once the service has been told its refund failed.
    for (const leg of live) {
      if (w.sim.byKey(leg.key)?.failureKnown) return `after ${after}: leg ${leg.key} is still ${leg.state} though its refund is known to have failed`;
    }
    return null;
  };

  for (const move of moves) {
    if (move.step === 'sweep') {
      await sweep(sweepAgeMs);
      const unsafe = safety('SWEEP');
      if (unsafe) return unsafe;
      continue;
    }
    const index = move.actor === 'L0' ? 0 : 1;
    switch (move.step) {
      case 'plan': {
        const plan = await w.ledger.planCashout(USER, credits(LEG_NET[index]), { requestId: move.actor, windowMs: 90 * DAY_MS });
        if (!plan.ok) return `plan refused: ${plan.reason}`;
        const [leg] = plan.legs;
        legs.set(move.actor, { key: leg!.key, net: LEG_NET[index], gross: Number(leg!.grossCents) });
        break;
      }
      // Stripe is idempotent per key: if the sweep already made this leg's
      // refund, the request's own call gets that same refund back.
      case 'succeeds':
      case 'refused':
      case 'failsAtOnce': {
        const leg = legs.get(move.actor)!;
        let refund = w.sim.byKey(leg.key);
        if (!refund && move.step === 'refused') {
          await w.ledger.reverseCashoutLeg(leg.key, 'stripe_refused');
          break;
        }
        refund ??= w.sim.make(`leg:${leg.key}`, leg.gross, leg.key, move.step === 'failsAtOnce' ? 'failed' : 'succeeded');
        if (refund.status === 'failed') refund.failureKnown = true;
        // What the request path does with Stripe's answer.
        if (refund.status === 'succeeded') await w.ledger.confirmCashoutLeg(leg.key, refund.id);
        else await w.ledger.reverseCashoutLeg(leg.key, 'refund_failed', { refundId: refund.id });
        break;
      }
      case 'lands': {
        const leg = legs.get(move.actor)!;
        if (!w.sim.byKey(leg.key)) w.sim.make(`leg:${leg.key}`, leg.gross, leg.key);
        break;
      }
      case 'dashRefund':
        dash = w.sim.make('D', DASH_CENTS);
        break;
      case 'snapshot': {
        const e = eventOf(move.actor, 'snapshot');
        if (e) await w.deliver(e);
        break;
      }
      case 'bankFails':
        w.sim.bankFails(move.actor === 'D' ? 'D' : `leg:${legs.get(move.actor)!.key}`, refundOf(move.actor));
        break;
      case 'failureEvent': {
        const e = eventOf(move.actor, 'failure');
        if (e) await w.deliver(e);
        break;
      }
    }
    const unsafe = safety(`${move.actor}.${move.step}`);
    if (unsafe) return unsafe;
  }

  // Quiescence: everything Stripe emitted arrives (in the order it was
  // emitted), the sweep settles what is still pending, and what the sweep
  // caused arrives too.
  for (let round = 0; round < 3; round++) {
    for (const e of w.sim.events.filter((x) => !x.delivered)) {
      await w.deliver(e);
      const unsafe = safety('a late event');
      if (unsafe) return unsafe;
    }
    await sweep(sweepAgeMs);
    const unsafe = safety('the final sweep');
    if (unsafe) return unsafe;
  }
  for (const e of w.sim.events.filter((x) => !x.delivered)) await w.deliver(e);

  // The truth, from the simulator: which refunds reached the card.
  let refundedNet = 0;
  for (const leg of legs.values()) {
    if (w.sim.byKey(leg.key)?.status === 'succeeded') refundedNet += leg.net;
  }
  if (dash?.status === 'succeeded') refundedNet += DASH_NET;
  const charge = w.ledger.charge(PI)!;
  const problems: string[] = [];
  const balance = w.ledger.availableMicro(USER);
  if (balance !== credits(BOUGHT - refundedNet)) {
    problems.push(`balance ${balance} != ${credits(BOUGHT - refundedNet)} (cash refunded ${w.sim.refundedNow} cents)`);
  }
  if (charge.refundedGrossCents !== BigInt(w.sim.refundedNow)) {
    problems.push(`ledger refundedGross ${charge.refundedGrossCents} != Stripe ${w.sim.refundedNow}`);
  }
  if (charge.refundedNetMicro !== credits(refundedNet)) problems.push(`ledger refundedNet ${charge.refundedNetMicro}`);
  if (charge.stripeSeenCents > BigInt(w.sim.refundedNow)) {
    problems.push(`stripeSeen ${charge.stripeSeenCents} overstates Stripe's ${w.sim.refundedNow}`);
  }
  if (w.ledger.allCashoutLegs().some((l) => l.state === 'pending')) problems.push('a leg is still pending');
  return problems.length ? problems.join('; ') : null;
}

/** Run every ordering; return how many ran and the first failures. */
async function walkAll(orderings: Move[][], sweepAges: number[]): Promise<{ ran: number; failures: string[] }> {
  let ran = 0;
  const failures: string[] = [];
  for (const moves of orderings) {
    for (const age of sweepAges) {
      ran += 1;
      const problem = await walk(moves, age);
      if (problem && failures.length < 5) {
        failures.push(`[sweep age ${Math.round(age / 3_600_000)} h] ${describeMoves(moves)}  =>  ${problem}`);
      }
    }
  }
  return { ran, failures };
}

/** A sweep inside Stripe's idempotency window (it may make the refund again
 *  under the same key) and one past it (it may only confirm or reverse). */
const SWEEP_AGES = [60 * 60 * 1000, REPOST_WITHIN_MS + 60 * 60 * 1000];

describe('the refund invariant: credits debited == cash actually refunded', () => {
  it('ONE leg: every life of a leg, a sweep at every position, both sweep ages', async () => {
    const orderings = LEG_CHAINS.flatMap((steps) => withSweeps(chain('L0', steps)));
    const { ran, failures } = await walkAll(orderings, SWEEP_AGES);
    expect(failures).toEqual([]);
    expect(ran).toBe(orderings.length * 2);
    expect(ran).toBe(106);
  });

  it('TWO legs on one charge: every interleaving of every pair of lives, a sweep at every position', async () => {
    const orderings: Move[][] = [];
    for (const a of LEG_CHAINS) {
      for (const b of LEG_CHAINS) {
        for (const moves of interleave(chain('L0', a), chain('L1', b))) orderings.push(...withSweeps(moves));
      }
    }
    // The sweep inside the idempotency window, at every position.
    const young = await walkAll(orderings, [SWEEP_AGES[0]!]);
    expect(young.failures).toEqual([]);
    // Past the window the sweep can only confirm or reverse: every interleaving, sweep at the end.
    const plain = orderings.filter((moves) => !moves.some((m) => m.step === 'sweep'));
    const old = await walkAll(plain, [SWEEP_AGES[1]!]);
    expect(old.failures).toEqual([]);
    expect(young.ran + old.ran).toBe(TWO_LEG_COUNT);
  }, 120_000);

  it('ONE leg and a refund made in the dashboard: every interleaving, a sweep at every position', async () => {
    const orderings: Move[][] = [];
    for (const a of LEG_CHAINS) {
      for (const d of DASH_CHAINS) {
        for (const moves of interleave(chain('L0', a), chain('D', d))) orderings.push(...withSweeps(moves));
      }
    }
    const young = await walkAll(orderings, [SWEEP_AGES[0]!]);
    expect(young.failures).toEqual([]);
    const plain = orderings.filter((moves) => !moves.some((m) => m.step === 'sweep'));
    const old = await walkAll(plain, [SWEEP_AGES[1]!]);
    expect(old.failures).toEqual([]);
    expect(young.ran + old.ran).toBe(DASH_COUNT);
  }, 60_000);
});

/** Pinned so a change to the table is a conscious one. */
const TWO_LEG_COUNT = 122_256 + 11_690;
const DASH_COUNT = 29_643;

describe('the two sequences the review found (now also rows of the table)', () => {
  it('(a) a failed leg, a NEW pending leg, then a stale snapshot equal to the new leg: nothing is debited when the new leg is refused', async () => {
    const w = await world();
    // L1: made, confirmed, fails at the bank; the failure event arrives.
    const p1 = await w.ledger.planCashout(USER, credits(400), { requestId: 'r1', windowMs: 90 * DAY_MS });
    if (!p1.ok) throw new Error('plan');
    const r1 = w.sim.make('leg:r1', 480, p1.legs[0]!.key);
    await w.ledger.confirmCashoutLeg(p1.legs[0]!.key, r1.id);
    w.sim.bankFails('leg:r1', r1);
    await w.deliver(w.sim.events.find((e) => e.kind === 'failure')!);
    expect(w.ledger.charge(PI)).toMatchObject({ refundFailures: 1, stripeSeenCents: 0n, refundedGrossCents: 0n });
    // L2: a new leg for the same amount is planned and is still pending.
    const p2 = await w.ledger.planCashout(USER, credits(400), { requestId: 'r2', windowMs: 90 * DAY_MS });
    if (!p2.ok) throw new Error('plan');
    expect(w.ledger.charge(PI)).toMatchObject({ refundedGrossCents: 480n });
    // The STALE charge.refunded from L1's refund (480) arrives only now: it
    // equals what the ledger has debited, so it looks explained. It is not.
    await w.deliver(w.sim.events.find((e) => e.kind === 'snapshot')!);
    expect(w.ledger.charge(PI)!.stripeSeenCents).toBe(0n);
    // Stripe refuses L2: reversed. The re-check must find nothing to debit.
    await w.ledger.reverseCashoutLeg(p2.legs[0]!.key, 'stripe_refused');
    expect(w.ledger.availableMicro(USER)).toBe(credits(1000));
  });

  it('(b) the sweep finds a refund that was seen by Stripe and then FAILED: the credits return once, and the late failure event changes nothing', async () => {
    const w = await world();
    const plan = await w.ledger.planCashout(USER, credits(400), { requestId: 'r1', windowMs: 90 * DAY_MS });
    if (!plan.ok) throw new Error('plan');
    const key = plan.legs[0]!.key;
    // The refund lands; the service dies before it hears. Its charge.refunded arrives.
    const refund = w.sim.make('leg:r1', 480, key);
    await w.deliver(w.sim.events.find((e) => e.kind === 'snapshot')!);
    expect(w.ledger.charge(PI)!.stripeSeenCents).toBe(480n);
    // It fails at the bank. The sweep runs BEFORE the failure event arrives.
    w.sim.bankFails('leg:r1', refund);
    await recoverPendingCashoutLegs({ ledger: w.ledger, stripe: w.sim.api(), onAlarm: () => {}, now: () => T0 + 3_600_000, graceMs: 0 });
    expect(w.ledger.cashoutLeg(key)).toMatchObject({ state: 'reversed' });
    expect(w.ledger.availableMicro(USER)).toBe(credits(1000));
    expect(w.ledger.charge(PI)).toMatchObject({ stripeSeenCents: 0n, refundFailures: 1, refundedGrossCents: 0n });
    // The failure event arrives late: already handled.
    await w.deliver(w.sim.events.find((e) => e.kind === 'failure')!);
    expect(w.ledger.availableMicro(USER)).toBe(credits(1000));
    expect((await w.store.load()).filter((l) => l.includes('"stripe-refund"') && !l.includes('"amount":"0"'))).toEqual([]);
  });

  it('(b, request path) Stripe answers a refund request with a FAILED refund after its charge.refunded was seen: no phantom debit', async () => {
    const w = await world();
    const credentials = await FiatCredentials.open(new MemoryLedgerStore());
    const token = await credentials.issue(USER);
    // Stripe makes the refund, emits charge.refunded, the refund fails, and only then answers.
    const api: StripeApi = {
      ...w.sim.api(),
      createRefund: async (request) => {
        const refund = w.sim.make('leg:req', request.amountCents!, request.idempotencyKey);
        await w.deliver(w.sim.events.find((e) => e.kind === 'snapshot' && !e.delivered)!);
        w.sim.bankFails('leg:req', refund);
        return { id: refund.id, status: 'failed' };
      },
    };
    const deps: CashoutDeps = { ledger: w.ledger, credentials, stripe: api, refundWindowDays: 90, now: () => T0, newRequestId: () => 'req', onAlarm: () => {} };
    const outcome = await requestCashout(deps, { credential: token, amountMicro: credits(400) });
    expect(outcome.status).toBe('stripe_error');
    expect(w.ledger.availableMicro(USER)).toBe(credits(1000));
    expect(w.ledger.charge(PI)).toMatchObject({ stripeSeenCents: 0n, refundFailures: 1 });
    await w.deliver(w.sim.events.find((e) => e.kind === 'failure')!);
    expect(w.ledger.availableMicro(USER)).toBe(credits(1000));
  });

  it('a dashboard refund that fails BEFORE its charge.refunded arrives is not paid for out of a live leg', async () => {
    const w = await world();
    const plan = await w.ledger.planCashout(USER, credits(400), { requestId: 'r1', windowMs: 90 * DAY_MS });
    if (!plan.ok) throw new Error('plan');
    const own = w.sim.make('leg:r1', 480, plan.legs[0]!.key);
    await w.ledger.confirmCashoutLeg(plan.legs[0]!.key, own.id);
    const dash = w.sim.make('D', 240);
    w.sim.bankFails('D', dash);
    await w.deliver(w.sim.events.find((e) => e.kind === 'failure')!);
    // The leg's 400 credits stay debited: its refund reached the card.
    expect(w.ledger.availableMicro(USER)).toBe(credits(600));
    for (const e of w.sim.events.filter((x) => !x.delivered)) await w.deliver(e);
    expect(w.ledger.availableMicro(USER)).toBe(credits(600));
  });
});

describe('R2 an unreadable PaymentIntent is not answered 503 for ever', () => {
  const dispute = (createdS: number) => ({
    id: 'evt_d',
    type: 'charge.dispute.created',
    created: createdS,
    data: { object: { id: 'dp_1', object: 'dispute', amount: 900, payment_intent: 'pi_other', status: 'needs_response' } },
  });

  async function deps(error: Error, nowMs: number) {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    const alarms: string[] = [];
    const api: StripeApi = {
      createRefund: async () => ({ id: 're' }),
      cardCountry: async () => 'GB',
      findRefund: async () => undefined,
      paymentState: async () => {
        throw error;
      },
    };
    const d: StripeWebhookDeps = { ledger, stripe: api, onAlarm: (m) => alarms.push(m), allowedCountries: new Set(['GB']), log: () => {}, now: () => nowMs };
    return { d, alarms };
  }
  const S = 1_794_000_000;

  it('404 (no such PaymentIntent for this key) means not ours: 200 at once, no alarm', async () => {
    const { d, alarms } = await deps(new StripeApiError('No such payment_intent', 404, 'resource_missing'), S * 1000 + 60_000);
    const res = await handleStripeEvent(d, dispute(S));
    expect(res.status).toBe(200);
    expect(res.body).toMatchObject({ ignored: expect.any(String) });
    expect(alarms).toEqual([]);
  });

  it('any other read failure: 503 while the EVENT is younger than six hours, then an alarm naming it and 200', async () => {
    const young = await deps(new StripeApiError('timeout', undefined, undefined), S * 1000 + 5 * 3_600_000);
    expect((await handleStripeEvent(young.d, dispute(S))).status).toBe(503);
    expect(young.alarms).toEqual([]);

    const old = await deps(new StripeApiError('timeout', undefined, undefined), S * 1000 + 6 * 3_600_000 + 1000);
    const res = await handleStripeEvent(old.d, dispute(S));
    expect(res.status).toBe(200);
    expect(old.alarms).toHaveLength(1);
    expect(old.alarms[0]).toMatch(/evt_d/);
    expect(old.alarms[0]).toMatch(/pi_other/);
    expect(old.alarms[0]).toMatch(/dp_1/);
  });

  it('a 5xx from Stripe is a read failure, not a "not ours"', async () => {
    const { d } = await deps(new StripeApiError('stripe is down', 503, undefined), S * 1000 + 60_000);
    expect((await handleStripeEvent(d, dispute(S))).status).toBe(503);
  });
});
