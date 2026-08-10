// TRIMMED copy for the standalone fiat service.
//
// The website's src/lib/seed.ts also derives the user's S5 storage seed from
// their smart-account address via @fabstir/sdk-core's generateS5SeedFromAddress.
// That is a CLIENT concern (the browser derives its own storage seed); the fiat
// backend never derives S5 seeds, so we do NOT pull @fabstir/sdk-core into this
// service. The only thing the service path imports from here is the chain id
// (fiat-challenge.ts -> BASE_SEPOLIA_CHAIN_ID). Keep this file in sync with the
// website's constant; the seed-derivation helpers stay website-only.

export const BASE_SEPOLIA_CHAIN_ID = 84532;

// Self-contained fingerprint (no sdk-core); kept for parity with the website's
// seed.ts. Unused on the current service path but harmless and dependency-free.
export async function seedFingerprint(seedPhrase: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(seedPhrase));
  return [...new Uint8Array(digest)]
    .slice(0, 6)
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}
