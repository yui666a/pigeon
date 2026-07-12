import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { SearchResult } from "../types/mail";
import { useErrorStore } from "./errorStore";

interface SearchState {
  query: string;
  results: SearchResult[];
  searching: boolean;
  /** 検索結果リスト内の選択位置（j/kナビ用）。未選択は -1 */
  selectedIndex: number;
  search: (accountId: string, query: string) => Promise<void>;
  clearSearch: () => void;
  /** 選択位置を direction 分移動する。境界で止まる（ループしない） */
  moveSelection: (direction: 1 | -1) => void;
  setSelectedIndex: (index: number) => void;
}

export const useSearchStore = create<SearchState>((set, get) => ({
  query: "",
  results: [],
  searching: false,
  selectedIndex: -1,

  search: async (accountId, query) => {
    if (!query.trim()) {
      set({ query: "", results: [], searching: false, selectedIndex: -1 });
      return;
    }
    set({ query, searching: true, selectedIndex: -1 });
    try {
      const results = await invoke<SearchResult[]>("search_mails", {
        accountId,
        query,
      });
      set({ results, searching: false });
    } catch (e) {
      set({ results: [], searching: false });
      useErrorStore.getState().addError(String(e));
    }
  },

  clearSearch: () =>
    set({ query: "", results: [], searching: false, selectedIndex: -1 }),

  moveSelection: (direction) => {
    const { results, selectedIndex } = get();
    if (results.length === 0) return;
    // 未選択時は「次」で先頭を選択。「前」は存在しないので止まる
    if (selectedIndex === -1) {
      if (direction === 1) set({ selectedIndex: 0 });
      return;
    }
    const next = selectedIndex + direction;
    if (next < 0 || next >= results.length) return;
    set({ selectedIndex: next });
  },

  setSelectedIndex: (index) => set({ selectedIndex: index }),
}));
