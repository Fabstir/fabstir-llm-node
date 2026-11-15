// GP1 — card packs in pounds: the copied v2 modules are pinned by hash, and the
// prices this service's settings give are fixed here (plan section 0, SPEC R13).
import { describe, it, expect } from 'vitest';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  PACK_CREDITS, UK_VAT_BPS, CARD_COST_BPS, CARD_COST_FIXED_MINOR, USDC_REFILL_BPS, TREASURY_BPS,
  packsAt, packPriceAt, cardCreditsFor, creditsForPayment, isPack,
} from '../src/lib/card-pricing';
import { vatInMinor } from '../src/lib/credit-price';

const sha = (rel: string) => createHash('sha256').update(readFileSync(join(__dirname, '..', rel))).digest('hex');

describe('the copied v2 modules are unchanged', () => {
  it('credit-price.ts matches its pinned hash', () => {
    expect(sha('src/lib/credit-price.ts')).toBe('7b6fff7dc78262171dc3cd3a0cbca6a9c202e4b7b9ebf891974ad98f55b5b881');
  });
  it('fx-rule.ts matches its pinned hash', () => {
    expect(sha('src/lib/fx-rule.ts')).toBe('0e2c133128ef862d2f65fca77c8d36ba81eef51823873604727dbfd652959024');
  });
});

describe('the settings (R13, D4, D5, D8)', () => {
  it('are the agreed figures', () => {
    expect({ PACK_CREDITS, UK_VAT_BPS, CARD_COST_BPS, CARD_COST_FIXED_MINOR, USDC_REFILL_BPS, TREASURY_BPS }).toEqual({
      PACK_CREDITS: [500, 1000, 2500, 10000], UK_VAT_BPS: 2000, CARD_COST_BPS: 330, CARD_COST_FIXED_MINOR: 28,
      USDC_REFILL_BPS: 100, TREASURY_BPS: 0,
    });
  });
  it('isPack accepts only the four packs', () => {
    for (const c of PACK_CREDITS) expect(isPack(c)).toBe(true);
    for (const c of [0, 50, 499, 501, 1001, 100000, '500', 500.5, undefined]) expect(isPack(c)).toBe(false);
  });
});

describe('pack prices', () => {
  it('at 1.33: £5.10 / £9.85 / £24.08 / £95.26 with their VAT (plan section 0)', () => {
    expect(packsAt(1_330_000)).toEqual([
      { credits: 500, priceMinor: 510, vatMinor: 85 },
      { credits: 1000, priceMinor: 985, vatMinor: 164 },
      { credits: 2500, priceMinor: 2408, vatMinor: 401 },
      { credits: 10000, priceMinor: 9526, vatMinor: 1588 },
    ]);
  });
  it('at 1.31: £5.18 / £10.00 / £24.46 / £96.72 (the SPEC orientation table)', () => {
    expect(packsAt(1_310_000).map((p) => p.priceMinor)).toEqual([518, 1000, 2446, 9672]);
  });
  it('vatMinor is vatInMinor of the price', () => {
    for (const p of packsAt(1_320_000)) expect(p.vatMinor).toBe(vatInMinor(p.priceMinor, 2000));
  });
});

describe('a pack price credits exactly the pack at every whole-cent rate from 1.00 to 1.40', () => {
  for (let cents = 100; cents <= 140; cents++) {
    const fx = cents * 10_000;
    it(`at ${(cents / 100).toFixed(2)}`, () => {
      for (const pack of PACK_CREDITS) {
        const price = packPriceAt(pack, fx);
        const vat = vatInMinor(price, UK_VAT_BPS);
        // The least price that buys the pack: a penny less does not.
        expect(creditsForPayment(price, vat, fx)).toBeGreaterThanOrEqual(pack);
        expect(creditsForPayment(price - 1, vatInMinor(price - 1, UK_VAT_BPS), fx)).toBeLessThan(pack);
        // And the webhook credits exactly the pack, never the overshoot.
        expect(cardCreditsFor({ pack, fxMicro: fx, priceMinor: price, amountTotal: price, amountTax: vat })).toEqual({
          credits: pack,
          exact: true,
        });
      }
    });
  }
});

describe('cardCreditsFor (A7)', () => {
  const fx = 1_330_000;
  it('Stripe VAT a penny either way still credits the pack', () => {
    expect(cardCreditsFor({ pack: 1000, fxMicro: fx, priceMinor: 985, amountTotal: 985, amountTax: 165 })).toEqual({ credits: 1000, exact: true });
    expect(cardCreditsFor({ pack: 1000, fxMicro: fx, priceMinor: 985, amountTotal: 985, amountTax: 163 })).toEqual({ credits: 1000, exact: true });
  });
  it('VAT two pence off is worked out from the payment and flagged', () => {
    const r = cardCreditsFor({ pack: 1000, fxMicro: fx, priceMinor: 985, amountTotal: 985, amountTax: 166 });
    expect(r.exact).toBe(false);
    expect(r.credits).toBeLessThan(1000);
  });
  it('zero VAT never hands the VAT out as credits: capped at the pack', () => {
    const r = cardCreditsFor({ pack: 1000, fxMicro: fx, priceMinor: 985, amountTotal: 985, amountTax: 0 });
    expect(r).toEqual({ credits: 1000, exact: false });
    expect(creditsForPayment(985, 0, fx)).toBeGreaterThan(1000);
  });
  it('the overshoot at 1.40 (501 for the 500 pack) is capped', () => {
    expect(creditsForPayment(488, 81, 1_400_000)).toBe(501);
    expect(cardCreditsFor({ pack: 500, fxMicro: 1_400_000, priceMinor: 488, amountTotal: 488, amountTax: 81 }).credits).toBe(500);
  });
  it('a short payment buys fewer credits', () => {
    const r = cardCreditsFor({ pack: 500, fxMicro: fx, priceMinor: 510, amountTotal: 400, amountTax: 67 });
    expect(r.exact).toBe(false);
    expect(r.credits).toBe(creditsForPayment(400, 67, fx));
    expect(r.credits).toBeLessThan(500);
  });
  it('a payment whose net is not positive buys nothing', () => {
    expect(creditsForPayment(20, 3, fx)).toBe(0);
  });
});
