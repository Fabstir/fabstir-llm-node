// Pure credit-price math (DC-2). NO chain-module imports — pure and synchronous,
// so tests need zero RPC mocking. `grossUpUsdc` is used by the server-side
// `depositFor` argument and the UI's USDC quote
// — one formula, so quote and charge can never disagree on the gross-up.
//
// A card customer pays in GBP pence (LAUNCH-MONEY-LIMITS DC-15): `packPriceMinor` is what a pack
// is charged, and the webhook credits `creditsForMinor(cardNetMinor(...))` of the amount actually
// paid, through the same net function, so a payment short of its price never buys the pack.

/** 100 credits = $1 — the FACE value of credits, not a card price. */
const CREDITS_PER_USD = 100;
/** 1 credit = 10_000 micro-USDC (6-dp base units): one credit is one cent (DC-2). The SDK's one statement of it. */
export const USDC_BASE_PER_CREDIT = 10_000;
/** USDC base units as display credits, exact (unrounded). */
export const usdcToCredits = (baseUnits: bigint): number => Number(baseUnits) / USDC_BASE_PER_CREDIT;
const BPS = 10_000n;

/** FACE value in USD of `credits`: exactly credits/100. What a price in credits is worth, and what
 *  a sale at that price pays out. It is NOT what a card customer pays for credits — that price carries the
 *  network fee and card costs: see {@link packPriceMinor}. */
export function netPriceUsd(credits: number): number {
  return credits / CREDITS_PER_USD;
}

/** Gross USDC (6-dp base units) to deposit so the buyer NETS exactly `credits`
 *  after the vault's treasury cut of `rateBps`.
 *
 *  FLOORS (never ceil): the contract's `treasury = gross * rate / BPS` also
 *  floors via Solidity integer division — the two floors cancel and the buyer
 *  nets an exact round number. E.g. 500 cr at 1000 bps →
 *  floor(5_000_000 × 10000/9000) = 5_555_555 deposited; the contract takes
 *  floor(5_555_555 × 1000/10000) = 555_555 → net 5_000_000 = exactly 500 cr.
 *  (Ceil would overshoot by 1 base unit → 500.0001 cr — an overcharge.) */
export function grossUpUsdc(credits: number, rateBps: number): bigint {
  assertRate(rateBps);
  const net = BigInt(credits * USDC_BASE_PER_CREDIT);
  const denom = BPS - BigInt(rateBps);
  return (net * BPS) / denom;
}

function assertRate(rateBps: number): void { assertBps('treasury rateBps', rateBps); }

function assertBps(what: string, bps: number): void {
  if (!Number.isInteger(bps) || bps < 0 || bps >= 10_000) throw new RangeError(`${what} must be an integer in [0, 10000): got ${bps}`);
}

function assertFx(fxMicro: number): void {
  if (!Number.isSafeInteger(fxMicro) || fxMicro <= 0) throw new RangeError(`fxMicro must be a positive safe integer: got ${fxMicro}`);
}

function assertAmount(what: string, amount: number): void {
  if (!Number.isSafeInteger(amount) || amount < 0) {
    throw new RangeError(`${what} must be a non-negative safe integer: got ${amount}`);
  }
}

// ── Card top-ups in GBP pence (DC-15): VAT, then card costs, then the USDC spread, then the fee. Every figure is a parameter.
const E8 = 10n ** 8n, E10 = 10n ** 10n;
const PACK_PRICE_MAX_STEPS = 64;
/** No price within {@link PACK_PRICE_MAX_STEPS} pennies of the closed-form start buys the pack. */
export class PackPriceNotFoundError extends RangeError {}
const ceilDiv = (a: bigint, b: bigint): bigint => (a + b - 1n) / b;

/** The VAT inside a VAT-inclusive `gross`, rounded half up: gross × vat / (10000 + vat). */
export function vatInMinor(gross: number, vatBps: number): number {
  assertAmount('gross', gross); assertBps('vatBps', vatBps);
  const d = BPS + BigInt(vatBps);
  return Number((2n * BigInt(gross) * BigInt(vatBps) + d) / (2n * d));
}

/** Credits a net of `netMinor` pence buys, floored; `fxMicro` = US dollars per pound × 10^6. */
export function creditsForMinor(netMinor: number, rateBps: number, fxMicro: number): number {
  assertAmount('netMinor', netMinor); assertRate(rateBps); assertFx(fxMicro);
  return Number((BigInt(netMinor) * BigInt(fxMicro) * (BPS - BigInt(rateBps))) / E10);
}

/** Card costs built into the price, at the dearest card accepted: ceil(gross × cost) + fixed. */
export function cardCostAllowanceMinor(gross: number, costBps: number, costFixedMinor: number): number {
  assertAmount('gross', gross); assertBps('costBps', costBps); assertAmount('costFixedMinor', costFixedMinor);
  return Number(ceilDiv(BigInt(gross) * BigInt(costBps), BPS)) + costFixedMinor;
}

/** The GBP→USDC refill spread on the credits' share only, rounded up; 0 when n0 ≤ 0. */
export function refillSpreadMinor(n0: number, rateBps: number, refillBps: number): number {
  if (!Number.isSafeInteger(n0)) throw new RangeError(`n0 must be a safe integer: got ${n0}`);
  assertRate(rateBps); assertBps('refillBps', refillBps);
  return n0 <= 0 ? 0 : Number(ceilDiv(BigInt(n0) * (BPS - BigInt(rateBps)) * BigInt(refillBps), E8));
}

/** The one net function: N0 = gross − tax − allowance, then N0 less the spread (N0 ≤ 0 comes back as is). */
export function cardNetMinor(gross: number, taxMinor: number, rateBps: number, costBps: number, costFixedMinor: number, refillBps: number): number {
  assertAmount('taxMinor', taxMinor);
  const n0 = gross - taxMinor - cardCostAllowanceMinor(gross, costBps, costFixedMinor);
  return n0 - refillSpreadMinor(n0, rateBps, refillBps);
}

type PackPriceArgs = [credits: number, rateBps: number, fxMicro: number, vatBps: number, costBps: number, costFixedMinor: number, refillBps: number];
/** The least whole gross whose net buys `credits`: a closed-form start, then up a penny at a time (`steps` pennies). */
export function packPriceSearch(...[credits, rateBps, fxMicro, vatBps, costBps, costFixedMinor, refillBps]: PackPriceArgs): { priceMinor: number; steps: number } {
  if (!Number.isSafeInteger(credits) || credits < 1) throw new RangeError(`a pack buys a positive whole number of credits: got ${credits}`);
  assertRate(rateBps); assertBps('vatBps', vatBps); assertBps('costBps', costBps); assertBps('refillBps', refillBps);
  assertFx(fxMicro); assertAmount('costFixedMinor', costFixedMinor);
  const keep = BPS - BigInt(rateBps), vatD = BPS + BigInt(vatBps), netShare = E8 - BigInt(costBps) * vatD;
  if (netShare <= 0n) throw new RangeError(`card costs of ${costBps} bps leave no net at ${vatBps} bps VAT`);
  const n0Min = ceilDiv(ceilDiv(BigInt(credits) * E10, BigInt(fxMicro) * keep) * E8, E8 - keep * BigInt(refillBps));
  const start = Math.max(1, Number(((2n * n0Min + 2n * BigInt(costFixedMinor) - 1n) * vatD * BPS) / (2n * netShare)) - 1);
  for (let steps = 0; steps <= PACK_PRICE_MAX_STEPS; steps++) {
    const price = start + steps, net = cardNetMinor(price, vatInMinor(price, vatBps), rateBps, costBps, costFixedMinor, refillBps);
    if (net > 0 && creditsForMinor(net, rateBps, fxMicro) >= credits) return { priceMinor: price, steps };
  }
  throw new PackPriceNotFoundError(`no price buys ${credits} credits within ${PACK_PRICE_MAX_STEPS} pennies of the start`);
}

/** A pack's card price in pence (DC-15). */
export const packPriceMinor = (...args: PackPriceArgs): number => packPriceSearch(...args).priceMinor;
