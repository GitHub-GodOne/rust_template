import { create } from "zustand";
import {
  fetchPreference,
  resetPreference,
  savePreference,
} from "../api/preferences";
import {
  DEFAULT_DOCK_CONFIG,
  type DockConfig,
  dockReservesSpaceForPath,
  normalizeDockConfig,
} from "../layouts/dock/dockConfig";

const PREFERENCE_KEY = "dock";
const cacheKey = (userKey: string) => `gpt-images-admin-dock:${userKey}`;

function readCache(userKey: string) {
  try {
    const raw = localStorage.getItem(cacheKey(userKey));
    return raw ? normalizeDockConfig(JSON.parse(raw)) : null;
  } catch {
    return null;
  }
}

function writeCache(userKey: string, config: DockConfig) {
  localStorage.setItem(cacheKey(userKey), JSON.stringify(config));
}

type DockState = {
  config: DockConfig;
  userKey: string | null;
  loaded: boolean;
  /** Loads the signed-in user's dock; the local cache shows instantly. */
  load: (userKey: string) => Promise<void>;
  save: (config: DockConfig) => Promise<void>;
  reset: () => Promise<void>;
};

export const useDockStore = create<DockState>((set, get) => ({
  config: DEFAULT_DOCK_CONFIG,
  userKey: null,
  loaded: false,
  load: async (userKey) => {
    if (get().userKey === userKey && get().loaded) return;
    set({
      userKey,
      loaded: false,
      config: readCache(userKey) ?? DEFAULT_DOCK_CONFIG,
    });
    try {
      const record = await fetchPreference<unknown>(PREFERENCE_KEY);
      if (get().userKey !== userKey) return;
      const config =
        record.value === null
          ? DEFAULT_DOCK_CONFIG
          : normalizeDockConfig(record.value);
      writeCache(userKey, config);
      set({ config, loaded: true });
    } catch {
      // Keep the cached/default dock when the server is unreachable.
      set({ loaded: true });
    }
  },
  save: async (config) => {
    const normalized = normalizeDockConfig(config);
    const record = await savePreference(PREFERENCE_KEY, normalized);
    const saved = normalizeDockConfig(record.value);
    const userKey = get().userKey;
    if (userKey) writeCache(userKey, saved);
    set({ config: saved });
  },
  reset: async () => {
    await resetPreference(PREFERENCE_KEY);
    const userKey = get().userKey;
    if (userKey) writeCache(userKey, DEFAULT_DOCK_CONFIG);
    set({ config: DEFAULT_DOCK_CONFIG });
  },
}));

/** Whether a bottom dock is permanently on screen on `pathname` (layouts leave room for it). */
export function useDockReservesBottom(dockVisible: boolean, pathname: string) {
  return useDockStore(
    (state) =>
      dockVisible &&
      state.config.enabled &&
      state.config.items.length > 0 &&
      state.config.position === "bottom" &&
      dockReservesSpaceForPath(state.config, pathname),
  );
}
