import { invokeCommand } from "./client";
import type { Draft, SaveDraftRequest } from "../types/mail";

/** 下書き系 Tauri commands の型付きラッパ */
export const draftApi = {
  fetchDrafts: (accountId: string) =>
    invokeCommand<Draft[]>("get_drafts", { accountId }),

  saveDraft: (req: SaveDraftRequest) =>
    invokeCommand<Draft>("save_draft", { req }),

  deleteDraft: (id: string) => invokeCommand<void>("delete_draft", { id }),
};
