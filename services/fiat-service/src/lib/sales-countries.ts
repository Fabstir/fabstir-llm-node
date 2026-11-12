// TX1 §3/§6 — where we sell. The United Kingdom at launch; the 27 European
// Union member states from the day SALES_EU=on (set on the same day as the EU
// VAT registration and the widened Radar rule, plan §6a). The webhook's
// country gate is the enforcement; the account page's selector reads the same
// switch on the site and is only presentation.

/** ISO 3166-1 alpha-2 codes of the 27 EU member states. */
export const EU_COUNTRIES: readonly string[] = [
  'AT', 'BE', 'BG', 'HR', 'CY', 'CZ', 'DK', 'EE', 'FI', 'FR', 'DE', 'GR', 'HU', 'IE',
  'IT', 'LV', 'LT', 'LU', 'MT', 'NL', 'PL', 'PT', 'RO', 'SK', 'SI', 'ES', 'SE',
];

export function euSalesEnabled(env: Record<string, string | undefined> = process.env): boolean {
  return env.SALES_EU === 'on';
}

/** The billing countries a purchase is credited for. Read per event, so the
 *  switch needs a restart at most and never a rebuild. */
export function allowedBillingCountries(env: Record<string, string | undefined> = process.env): Set<string> {
  return new Set(euSalesEnabled(env) ? ['GB', ...EU_COUNTRIES] : ['GB']);
}
