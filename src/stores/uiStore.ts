import { create } from "zustand";

export type ViewMode = "threads" | "unclassified" | "project";

interface UiState {
  viewMode: ViewMode;
  setViewMode: (mode: ViewMode) => void;
}

export const useUiStore = create<UiState>((set) => ({
  viewMode: "threads",
  setViewMode: (mode) => set({ viewMode: mode }),
}));
