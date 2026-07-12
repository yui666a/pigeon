import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Draft, SaveDraftRequest } from "../types/mail";
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
      const drafts = await invoke<Draft[]>("get_drafts", {
        accountId,
      });
      set({ drafts: drafts ?? [], loading: false });
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(String(e));
    }
  },

  saveDraft: async (req) => {
    try {
      return await invoke<Draft>("save_draft", { req });
    } catch (e) {
      // 下書き保存はベストエフォート。失敗を理由に閉じる等の操作を妨げない
      useErrorStore.getState().addError(String(e));
      return null;
    }
  },

  deleteDraft: async (id) => {
    try {
      await invoke("delete_draft", { id });
      set({ drafts: get().drafts.filter((d) => d.id !== id) });
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  },
}));
