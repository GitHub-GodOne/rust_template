import { create } from "zustand";

export type ThemeMode = "light" | "dark" | "system";
export type ResolvedTheme = "light" | "dark";

const storageKey = "gpt-images-admin-theme";
const media =
  typeof window !== "undefined" && window.matchMedia
    ? window.matchMedia("(prefers-color-scheme: dark)")
    : null;

function readMode(): ThemeMode {
  try {
    const stored = localStorage.getItem(storageKey);
    return stored === "light" || stored === "dark" || stored === "system"
      ? stored
      : "light";
  } catch {
    return "light";
  }
}

export function resolveTheme(
  mode: ThemeMode,
  systemDark: boolean,
): ResolvedTheme {
  if (mode === "system") return systemDark ? "dark" : "light";
  return mode;
}

function apply(theme: ResolvedTheme) {
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
}

type ThemeState = {
  mode: ThemeMode;
  resolved: ResolvedTheme;
  setMode: (mode: ThemeMode) => void;
  /** light → dark → system → light */
  cycle: () => void;
};

const initialMode = readMode();
const initialResolved = resolveTheme(initialMode, Boolean(media?.matches));
apply(initialResolved);

export const useThemeStore = create<ThemeState>((set, get) => ({
  mode: initialMode,
  resolved: initialResolved,
  setMode: (mode) => {
    try {
      localStorage.setItem(storageKey, mode);
    } catch {
      // Private mode: the choice lasts for this page only.
    }
    const resolved = resolveTheme(mode, Boolean(media?.matches));
    apply(resolved);
    set({ mode, resolved });
  },
  cycle: () => {
    const order: ThemeMode[] = ["light", "dark", "system"];
    const next = order[(order.indexOf(get().mode) + 1) % order.length];
    get().setMode(next);
  },
}));

media?.addEventListener("change", (event) => {
  const { mode } = useThemeStore.getState();
  if (mode !== "system") return;
  const resolved = resolveTheme(mode, event.matches);
  apply(resolved);
  useThemeStore.setState({ resolved });
});
