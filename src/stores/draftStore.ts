import { create } from "zustand";
import type { Draft, SaveDraftRequest } from "../types/mail";
import { draftApi } from "../api/draftApi";
import { errorMessage } from "../api/errors";
import { useErrorStore } from "./errorStore";

interface DraftState {
  drafts: Draft[];
  loading: boolean;
  fetchDrafts: (accountId: string) => Promise<void>;
  /** 保存に失敗しても呼び出し側の操作（compose閉じる等）を止めない。失敗時は null を返す */
  saveDraft: (req: SaveDraftRequest) => Promise<Draft | null>;
  deleteDraft: (id: string) => Promise<void>;
}

export const useDraftStore = create<DraftState>((set, get) => ({
  drafts: [],
  loading: false,

  fetchDrafts: async (accountId) => {
    set({ loading: true });
    try {
      const drafts = await draftApi.fetchDrafts(accountId);
      set({ drafts: drafts ?? [], loading: false });
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  saveDraft: async (req) => {
    try {
      const saved = await draftApi.saveDraft(req);
      // 一覧(drafts)にも反映する。既存idなら置換、新規なら updated_at 降順を保って挿入。
      // これをしないと自動保存直後に一覧を開いても再マウントまで反映されない
      const rest = get().drafts.filter((d) => d.id !== saved.id);
      const insertAt = rest.findIndex((d) => d.updated_at < saved.updated_at);
      const drafts =
        insertAt === -1
          ? [...rest, saved]
          : [...rest.slice(0, insertAt), saved, ...rest.slice(insertAt)];
      set({ drafts });
      return saved;
    } catch (e) {
      // 下書き保存はベストエフォート。失敗を理由に閉じる等の操作を妨げない
      useErrorStore.getState().addError(errorMessage(e));
      return null;
    }
  },

  deleteDraft: async (id) => {
    try {
      await draftApi.deleteDraft(id);
      set({ drafts: get().drafts.filter((d) => d.id !== id) });
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },
}));
