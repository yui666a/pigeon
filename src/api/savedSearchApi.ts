import { invokeCommand } from "./client";
import type { SavedSearch } from "../types/savedSearch";
import type { SearchMode } from "../types/search";

/** 保存検索（スマートビュー）CRUD 系 Tauri commands の型付きラッパ */
export const savedSearchApi = {
  list: () => invokeCommand<SavedSearch[]>("list_saved_searches", {}),

  create: (name: string, query: string, mode: SearchMode) =>
    invokeCommand<SavedSearch>("create_saved_search", { name, query, mode }),

  rename: (id: number, name: string) =>
    invokeCommand<void>("rename_saved_search", { id, name }),

  remove: (id: number) => invokeCommand<void>("delete_saved_search", { id }),
};
