import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { useErrorStore } from "./errorStore";
import { useProjectStore } from "./projectStore";
import type { ClassifyResponse } from "../types/classifier";
import type { Project } from "../types/project";

interface UnclassifiedMailRef {
  id: string;
}

interface ClassifyState {
  classifying: boolean;
  progress: { current: number; total: number } | null;
  pendingProposal: ClassifyResponse | null;
  // 内部: 逐次ループの状態
  _queue: UnclassifiedMailRef[];
  _index: number;
  _cancelled: boolean;

  classifyMail: (mailId: string) => Promise<void>;
  classifyAll: (accountId: string) => Promise<void>;
  cancelClassification: () => Promise<void>;
  approveNewProject: (
    mailId: string,
    projectName: string,
    description?: string,
  ) => Promise<void>;
  rejectClassification: (mailId: string) => Promise<void>;
}

export const useClassifyStore = create<ClassifyState>((set, get) => {
  // 次の1件を分類し、create でなければ自動で次へ進む
  const classifyNext = async (): Promise<void> => {
    const { _queue, _index, _cancelled } = get();
    if (_cancelled || _index >= _queue.length) {
      set({ classifying: false, progress: null, pendingProposal: null });
      return;
    }
    const mail = _queue[_index];
    let res: ClassifyResponse;
    try {
      // classify_mail は Rust の ClassifyResponse を返す。mail_id と
      // ClassifyResult が両方とも #[serde(flatten)] されているため、
      // 実際のJSONは { mail_id, action, confidence, reason, ... } の
      // 完全にフラットな形になる（result という入れ子は存在しない）。
      const r = await invoke<ClassifyResponse>("classify_mail", {
        mailId: mail.id,
      });
      res = r;
    } catch (e) {
      useErrorStore.getState().addError(String(e));
      set({ classifying: false, progress: null });
      return;
    }
    set({
      _index: _index + 1,
      progress: { current: _index + 1, total: _queue.length },
    });
    if (res.action === "create") {
      set({ pendingProposal: res });
      return; // 停止：承認/却下を待つ
    }
    await classifyNext();
  };

  return {
    classifying: false,
    progress: null,
    pendingProposal: null,
    _queue: [],
    _index: 0,
    _cancelled: false,

    classifyMail: async (mailId) => {
      try {
        await invoke("classify_mail", { mailId });
      } catch (e) {
        useErrorStore.getState().addError(String(e));
      }
    },

    classifyAll: async (accountId) => {
      try {
        const mails = await invoke<UnclassifiedMailRef[]>(
          "get_unclassified_mails",
          { accountId },
        );
        set({
          classifying: true,
          _queue: mails,
          _index: 0,
          _cancelled: false,
          pendingProposal: null,
          progress: { current: 0, total: mails.length },
        });
        await classifyNext();
      } catch (e) {
        set({ classifying: false, progress: null });
        useErrorStore.getState().addError(String(e));
      }
    },

    cancelClassification: async () => {
      set({ _cancelled: true, classifying: false, progress: null, pendingProposal: null });
    },

    approveNewProject: async (mailId, projectName, description) => {
      try {
        const project = await invoke<Project>("approve_new_project", {
          mailId,
          projectName,
          description: description ?? null,
        });
        useProjectStore.getState().addProject(project);
        set({ pendingProposal: null });
        await classifyNext();
      } catch (e) {
        useErrorStore.getState().addError(String(e));
      }
    },

    rejectClassification: async (mailId) => {
      try {
        await invoke("reject_classification", { mailId });
      } catch (e) {
        useErrorStore.getState().addError(String(e));
      }
      set({ pendingProposal: null });
      await classifyNext();
    },
  };
});
