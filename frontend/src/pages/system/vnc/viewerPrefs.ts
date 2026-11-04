// Per-browser remote viewer preferences shared by the VNC and RDP pages.

export function storedFlag(key: string, fallback: () => boolean) {
  try {
    const value = localStorage.getItem(key);
    return value === null ? fallback() : value === "1";
  } catch {
    return fallback();
  }
}

export function storeFlag(key: string, value: boolean) {
  try {
    localStorage.setItem(key, value ? "1" : "0");
  } catch {
    // Private mode or blocked storage: the choice lasts for this page only.
  }
}

/** Touch-first devices start in trackpad mode. */
export function prefersTrackpad() {
  return (
    typeof window !== "undefined" &&
    Boolean(window.matchMedia?.("(pointer: coarse)").matches)
  );
}

export function storedValue(key: string, fallback: () => string) {
  try {
    return localStorage.getItem(key) ?? fallback();
  } catch {
    return fallback();
  }
}

export function storeValue(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Private mode or blocked storage: the choice lasts for this page only.
  }
}
