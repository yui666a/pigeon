import { invokeCommand } from "./client";
import type { SearchResult } from "../types/mail";

/** 全文検索系 Tauri commands の型付きラッパ */
export const searchApi = {
  searchMails: (accountId: string, query: string) =>
    invokeCommand<SearchResult[]>("search_mails", { accountId, query }),
  /** セマンティック検索。クエリの埋め込み生成は command 側で行われる（UI 配線は次段階） */
  semanticSearch: (accountId: string, query: string) =>
    invokeCommand<SearchResult[]>("semantic_search", { accountId, query }),
};
