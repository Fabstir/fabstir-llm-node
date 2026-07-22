// The FC1 identity scheme, locked in ONE place.
//
// `fc1UserId` is the user's SMART-ACCOUNT ADDRESS (their passkey-derived Base
// account), lowercased. It is the single identity that ties together:
//   - a browser card purchase        (POST /api/fiat/purchase → Stripe metadata)
//   - the Decision-9 credential       (issued for this id; the helper presents it)
//   - the helper's vault session-opens (POST /api/fiat/session)
//   - the ledger balance itself
// so a purchase tops up exactly the balance the helper later spends.
//
// Why the address: the website already derives it from the passkey login, it is
// stable and unique per user, and it needs no extra account system. Crediting a
// balance is harmless (the payer pays), so the purchase route may trust a
// client-supplied address; SPENDING requires a credential, whose issuance is the
// security boundary (operator-gated at launch; a self-serve flow must prove
// address ownership with a signature — a documented follow-up).
/** Normalise + validate an address into an fc1UserId (lowercased). A FORMAT
 *  check (0x + 20 bytes hex), not an EIP-55 checksum check — the address is
 *  accepted in any casing and lowercased, so the same user always maps to one
 *  id whether the client sends it checksummed or lowercase. Throws on a
 *  malformed address so a bad id can never silently split a user's balance. */
export function fiatUserId(address: string): string {
  if (typeof address !== 'string' || !/^0x[0-9a-fA-F]{40}$/.test(address)) {
    throw new Error(`fc1UserId must be a valid address, got "${address}"`);
  }
  return address.toLowerCase();
}
