import { invokeCommand } from "./client";
import type { SearchResult } from "../types/mail";

/** 全文検索系 Tauri commands の型付きラッパ */
export const searchApi = {
  /** projectId 指定時は選択案件のサブツリーに検索範囲を限定する（未分類メールは対象外） */
  searchMails: (accountId: string, query: string, projectId?: string) =>
    invokeCommand<SearchResult[]>("search_mails", { accountId, query, projectId }),
  /** セマンティック検索。クエリの埋め込み生成は command 側で行われる（UI 配線は次段階） */
  semanticSearch: (accountId: string, query: string, projectId?: string) =>
    invokeCommand<SearchResult[]>("semantic_search", { accountId, query, projectId }),
};
