import { create } from "zustand";

const contentZoomKey = "gpt-images-admin-content-zoom";
const zoomOptions = [
  0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.85, 0.9, 1, 1.1, 1.2, 1.35,
];

function readStoredZoom() {
  const value = Number(localStorage.getItem(contentZoomKey));
  return zoomOptions.includes(value) ? value : 1;
}

type UiState = {
  contentZoom: number;
  zoomIn: () => void;
  zoomOut: () => void;
  resetZoom: () => void;
};

function persistZoom(value: number) {
  localStorage.setItem(contentZoomKey, String(value));
  return value;
}

export const useUiStore = create<UiState>((set, get) => ({
  contentZoom: readStoredZoom(),
  zoomIn: () => {
    const current = get().contentZoom;
    const index = zoomOptions.findIndex((value) => value >= current);
    set({
      contentZoom: persistZoom(
        zoomOptions[Math.min(index + 1, zoomOptions.length - 1)],
      ),
    });
  },
  zoomOut: () => {
    const current = get().contentZoom;
    const index = zoomOptions.findIndex((value) => value >= current);
    set({ contentZoom: persistZoom(zoomOptions[Math.max(index - 1, 0)]) });
  },
  resetZoom: () => set({ contentZoom: persistZoom(1) }),
}));
