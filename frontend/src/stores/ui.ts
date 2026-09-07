import { create } from "zustand";

const contentZoomKey = "gpt-images-admin-content-zoom";
const sidebarCollapsedKey = "gpt-images-admin-sidebar-collapsed";
const sidebarOpenKeysKey = "gpt-images-admin-sidebar-open-keys";
const dockVisibleKey = "gpt-images-admin-dock-visible";
const zoomOptions = [0.7, 0.8, 0.9, 1];

function readStoredZoom() {
  const value = Number(localStorage.getItem(contentZoomKey));
  return zoomOptions.includes(value) ? value : 1;
}

function readStoredSidebarCollapsed() {
  return localStorage.getItem(sidebarCollapsedKey) === "true";
}

function readStoredDockVisible() {
  return localStorage.getItem(dockVisibleKey) !== "false";
}

function readStoredSidebarOpenKeys() {
  const value = localStorage.getItem(sidebarOpenKeysKey);
  if (!value) {
    return {};
  }

  try {
    const parsed = JSON.parse(value) as Record<string, unknown>;
    return Object.fromEntries(
      Object.entries(parsed).flatMap(([userId, keys]) =>
        Array.isArray(keys) && keys.every((key) => typeof key === "string")
          ? [[userId, [...new Set(keys)]]]
          : [],
      ),
    ) as Record<string, string[]>;
  } catch {
    localStorage.removeItem(sidebarOpenKeysKey);
    return {};
  }
}

type UiState = {
  contentZoom: number;
  sidebarCollapsed: boolean;
  sidebarOpenKeysByUser: Record<string, string[]>;
  dockVisible: boolean;
  zoomIn: () => void;
  zoomOut: () => void;
  resetZoom: () => void;
  toggleSidebarCollapsed: () => void;
  setSidebarOpenKeys: (userId: string, keys: string[]) => void;
  toggleDockVisible: () => void;
};

function persistZoom(value: number) {
  localStorage.setItem(contentZoomKey, String(value));
  return value;
}

function currentZoomIndex(current: number) {
  const index = zoomOptions.findIndex((value) => value >= current);
  return index === -1 ? zoomOptions.length - 1 : index;
}

export const useUiStore = create<UiState>((set, get) => ({
  contentZoom: readStoredZoom(),
  sidebarCollapsed: readStoredSidebarCollapsed(),
  sidebarOpenKeysByUser: readStoredSidebarOpenKeys(),
  dockVisible: readStoredDockVisible(),
  zoomIn: () => {
    const current = get().contentZoom;
    const index = currentZoomIndex(current);
    set({
      contentZoom: persistZoom(
        zoomOptions[Math.min(index + 1, zoomOptions.length - 1)],
      ),
    });
  },
  zoomOut: () => {
    const current = get().contentZoom;
    const index = currentZoomIndex(current);
    set({ contentZoom: persistZoom(zoomOptions[Math.max(index - 1, 0)]) });
  },
  resetZoom: () => set({ contentZoom: persistZoom(1) }),
  toggleSidebarCollapsed: () =>
    set((state) => {
      const sidebarCollapsed = !state.sidebarCollapsed;
      localStorage.setItem(sidebarCollapsedKey, String(sidebarCollapsed));
      return { sidebarCollapsed };
    }),
  setSidebarOpenKeys: (userId, keys) =>
    set((state) => {
      const sidebarOpenKeysByUser = {
        ...state.sidebarOpenKeysByUser,
        [userId]: [...new Set(keys)],
      };
      localStorage.setItem(
        sidebarOpenKeysKey,
        JSON.stringify(sidebarOpenKeysByUser),
      );
      return { sidebarOpenKeysByUser };
    }),
  toggleDockVisible: () =>
    set((state) => {
      const dockVisible = !state.dockVisible;
      localStorage.setItem(dockVisibleKey, String(dockVisible));
      return { dockVisible };
    }),
}));
