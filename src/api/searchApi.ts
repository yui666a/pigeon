import { invokeCommand } from "./client";
import type { SearchResult } from "../types/mail";

/** 全文検索系 Tauri commands の型付きラッパ */
export const searchApi = {
  searchMails: (accountId: string, query: string) =>
    invokeCommand<SearchResult[]>("search_mails", { accountId, query }),
};
