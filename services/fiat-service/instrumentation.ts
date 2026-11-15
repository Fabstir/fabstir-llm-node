// Next.js instrumentation hook, inside the server process.
//
// GP1b: the card exchange-rate feed starts ALWAYS (it is what prices card
// packs and what the webhook judges R14 against). A misconfigured setting puts
// it in a state that serves no rate (card top-ups answer 503) with an urgent
// alert; every other route runs as before. It never stops the boot.
//
// FC1.3: the settlement listener, inert unless FIAT_SETTLEMENT_ENABLED=1.
export async function register(): Promise<void> {
  if (process.env.NEXT_RUNTIME && process.env.NEXT_RUNTIME !== 'nodejs') return;
  const { getCardFx } = await import('./src/lib/card-fx');
  await getCardFx().catch(() => undefined); // logged by getCardFx; routes retry the build
  if (process.env.FIAT_SETTLEMENT_ENABLED !== '1') return;
  const { startProductionSettlementListener } = await import('./src/lib/settlement-listener');
  await startProductionSettlementListener();
}
