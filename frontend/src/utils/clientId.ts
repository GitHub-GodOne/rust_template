let fallbackSequence = 0;

function randomHex() {
  const webCrypto = globalThis.crypto;
  if (typeof webCrypto?.randomUUID === "function") {
    return webCrypto.randomUUID();
  }

  if (typeof webCrypto?.getRandomValues === "function") {
    const values = new Uint32Array(4);
    webCrypto.getRandomValues(values);
    return Array.from(values, (value) =>
      value.toString(16).padStart(8, "0"),
    ).join("");
  }

  fallbackSequence += 1;
  return `${Date.now().toString(36)}-${fallbackSequence.toString(36)}-${Math.random()
    .toString(36)
    .slice(2)}`;
}

export function createClientId(prefix: string) {
  return `${prefix}-${randomHex()}`;
}
