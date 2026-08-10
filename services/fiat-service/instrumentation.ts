// Next.js instrumentation hook: starts the FC1.3 settlement listener inside
// the server process. Inert unless FIAT_SETTLEMENT_ENABLED=1 — with the flag
// unset the server behaves byte-identically to before this file existed.
export async function register(): Promise<void> {
  if (process.env.NEXT_RUNTIME && process.env.NEXT_RUNTIME !== 'nodejs') return;
  if (process.env.FIAT_SETTLEMENT_ENABLED !== '1') return;
  const { startProductionSettlementListener } = await import('./src/lib/settlement-listener');
  await startProductionSettlementListener();
}
