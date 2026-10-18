// Pure presentation helpers for the certificates page; import-free so the
// node tests can load them directly.

export type Tone = "success" | "processing" | "warning" | "error" | "default";

/** Public URL a mapping will answer on (443 is implied). */
export function previewUrl(
  domain: string,
  mode: string,
  publicPort?: number | null,
) {
  const host = domain.trim().toLowerCase() || "example.com";
  if (mode === "tunnel" || !publicPort || publicPort === 443) {
    return `https://${host}`;
  }
  return `https://${host}:${publicPort}`;
}

/** Local target, bracketing IPv6 literals. */
export function upstreamLabel(host?: string | null, port?: number | null) {
  const value = (host ?? "").trim() || "127.0.0.1";
  const shown = value.includes(":") ? `[${value}]` : value;
  return `http://${shown}:${port ?? ""}`;
}

/** Colour for the days left before a certificate expires. */
export function expiryTone(days?: number | null): Tone {
  if (days === null || days === undefined) {
    return "default";
  }
  if (days < 0) {
    return "error";
  }
  if (days <= 14) {
    return "warning";
  }
  return "success";
}

export function certificateTone(status: string, issuing = false): Tone {
  if (issuing || status === "issuing") {
    return "processing";
  }
  if (status === "active") {
    return "success";
  }
  if (status === "failed") {
    return "error";
  }
  return "default";
}

export function mappingTone(status: string): Tone {
  if (status === "active") {
    return "success";
  }
  if (status === "error") {
    return "error";
  }
  if (status === "pending") {
    return "warning";
  }
  return "default";
}

/** Ports below 1024 usually need extra privileges to listen on. */
export function isPrivilegedPort(port?: number | null) {
  return typeof port === "number" && port > 0 && port < 1024;
}

/** Splits free text (commas, spaces, new lines) into unique domain names. */
export function parseDomainList(value: string) {
  const names: string[] = [];
  for (const name of value.split(/[\s,，]+/)) {
    const normalized = name.trim().toLowerCase().replace(/\.$/, "");
    if (normalized && !names.includes(normalized)) {
      names.push(normalized);
    }
  }
  return names;
}
