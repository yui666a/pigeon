import { create } from "zustand";
import type { SearchResult } from "../types/mail";
import type { SearchMode } from "../types/search";
import { searchApi } from "../api/searchApi";
import { errorMessage } from "../api/errors";
import { useErrorStore } from "./errorStore";

/** 検索モードの永続化キー。保存先は既存の通知トグルと統一（localStorage 直読み書き）。 */
export const SEARCH_MODE_KEY = "pigeon.searchMode";

/** localStorage から検索モードを読む。不正値・未設定は "fulltext" にフォールバック。 */
function readPersistedMode(): SearchMode {
  const v = localStorage.getItem(SEARCH_MODE_KEY);
  return v === "semantic" ? "semantic" : "fulltext";
}

interface SearchState {
  /** 現在の検索モード（fulltext / semantic）。初期値は localStorage から復元 */
  mode: SearchMode;
  query: string;
  results: SearchResult[];
  searching: boolean;
  /** 検索結果リスト内の選択位置（j/kナビ用）。未選択は -1 */
  selectedIndex: number;
  search: (accountId: string, query: string) => Promise<void>;
  /** モードを変更し localStorage に永続化する。再検索は行わない（呼び出し側の責務） */
  setMode: (mode: SearchMode) => void;
  /** localStorage から永続化済みモードを読む（不正値は fulltext） */
  loadPersistedMode: () => SearchMode;
  clearSearch: () => void;
  /** 選択位置を direction 分移動する。境界で止まる（ループしない） */
  moveSelection: (direction: 1 | -1) => void;
  setSelectedIndex: (index: number) => void;
}

export const useSearchStore = create<SearchState>((set, get) => ({
  mode: readPersistedMode(),
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
      const searchFn =
        get().mode === "semantic"
          ? searchApi.semanticSearch
          : searchApi.searchMails;
      const results = await searchFn(accountId, query);
      set({ results, searching: false });
    } catch (e) {
      set({ results: [], searching: false });
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  setMode: (mode) => {
    localStorage.setItem(SEARCH_MODE_KEY, mode);
    set({ mode });
  },

  loadPersistedMode: () => readPersistedMode(),

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
