/** Returns the canonical (compressed, lower-case) form, or null if invalid. */
export function normalizeIpv6(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed.includes(":") || /[[\]/%\s]/.test(trimmed)) return null;
  try {
    // The URL parser implements the full IPv6 grammar, including `::` and
    // embedded IPv4 suffixes.
    return new URL(`http://[${trimmed}]/`).hostname.slice(1, -1);
  } catch {
    return null;
  }
}

/**
 * Mirrors the backend rule for administrator-supplied addresses: rejects
 * unspecified, loopback, link-local, multicast and IPv4-mapped addresses.
 */
export function isValidManualIpv6(value: string) {
  const address = normalizeIpv6(value);
  if (!address || address === "::" || address === "::1") return false;
  if (address.startsWith("::ffff:")) return false;
  const first = address.startsWith("::")
    ? 0
    : Number.parseInt(address.split(":")[0], 16);
  if ((first & 0xffc0) === 0xfe80) return false;
  if ((first & 0xff00) === 0xff00) return false;
  return true;
}
