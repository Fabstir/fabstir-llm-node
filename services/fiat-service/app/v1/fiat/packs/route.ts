// GP2 — GET /v1/fiat/packs: the card packs and their prices in pounds, VAT
// included, at the rate served now (plan 2.3). Public; CORS as the other
// browser routes (middleware.ts). The rate itself is not in the answer, only
// prices. `no-store`: a cached price would make the buyer meet R12's
// `price_changed` for no reason. No rate served: `503 card_unavailable`.
import { CARD_CURRENCY, PRICING_TAG, packsAt } from '../../../../src/lib/card-pricing';
import { getCardFx } from '../../../../src/lib/card-fx';

// Never prerendered at build time: the answer moves with the rate.
export const dynamic = 'force-dynamic';

const NO_STORE = { 'cache-control': 'no-store' };

export async function GET(): Promise<Response> {
  try {
    const answer = await (await getCardFx()).servedNow();
    if (answer.kind !== 'rate') throw new Error(answer.reason);
    return Response.json(
      { currency: CARD_CURRENCY, pricing: PRICING_TAG, packs: packsAt(answer.fxMicro) },
      { headers: NO_STORE }
    );
  } catch {
    return Response.json(
      { error: 'card_unavailable', message: 'card top-ups are briefly unavailable; try again later' },
      { status: 503, headers: NO_STORE }
    );
  }
}
