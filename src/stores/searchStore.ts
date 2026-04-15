import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { SearchResult } from "../types/mail";
import { useErrorStore } from "./errorStore";

interface SearchState {
  query: string;
  results: SearchResult[];
  searching: boolean;
  search: (accountId: string, query: string) => Promise<void>;
  clearSearch: () => void;
}

export const useSearchStore = create<SearchState>((set) => ({
  query: "",
  results: [],
  searching: false,

  search: async (accountId, query) => {
    if (!query.trim()) {
      set({ query: "", results: [], searching: false });
      return;
    }
    set({ query, searching: true });
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

  clearSearch: () => set({ query: "", results: [], searching: false }),
}));
