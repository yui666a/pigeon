import { create } from "zustand";
import { savedSearchApi } from "../api/savedSearchApi";
import { errorMessage } from "../api/errors";
import type { SavedSearch } from "../types/savedSearch";
import type { SearchMode } from "../types/search";
import { useErrorStore } from "./errorStore";

interface SavedSearchState {
  savedSearches: SavedSearch[];
  loading: boolean;
  fetch: () => Promise<void>;
  create: (name: string, query: string, mode: SearchMode) => Promise<void>;
  rename: (id: number, name: string) => Promise<void>;
  remove: (id: number) => Promise<void>;
}

/**
 * 保存検索（スマートビュー）の一覧と CRUD 操作を管理するストア。
 * 全 mutation は成功後に fetch() で再読込し、失敗は searchStore と同じ経路
 * （errorMessage 整形 + useErrorStore.addError）でトースト通知する。
 */
export const useSavedSearchStore = create<SavedSearchState>((set, get) => ({
  savedSearches: [],
  loading: false,

  fetch: async () => {
    set({ loading: true });
    try {
      set({ savedSearches: await savedSearchApi.list() });
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    } finally {
      set({ loading: false });
    }
  },

  create: async (name, query, mode) => {
    try {
      await savedSearchApi.create(name, query, mode);
      await get().fetch();
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  rename: async (id, name) => {
    try {
      await savedSearchApi.rename(id, name);
      await get().fetch();
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  remove: async (id) => {
    try {
      await savedSearchApi.remove(id);
      await get().fetch();
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },
}));
