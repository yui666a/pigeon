import { create } from "zustand";
import { classifyApi } from "../api/classifyApi";
import { errorMessage } from "../api/errors";
import { useErrorStore } from "./errorStore";
import { useProjectStore } from "./projectStore";
import type { ClassifyResponse, UnclassifiedMailRef } from "../types/classifier";

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
      res = await classifyApi.classifyMail(mail.id);
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
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
        await classifyApi.classifyMail(mailId);
      } catch (e) {
        useErrorStore.getState().addError(errorMessage(e));
      }
    },

    classifyAll: async (accountId) => {
      try {
        const mails = await classifyApi.fetchUnclassifiedMailRefs(accountId);
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
        useErrorStore.getState().addError(errorMessage(e));
      }
    },

    cancelClassification: async () => {
      set({ _cancelled: true, classifying: false, progress: null, pendingProposal: null });
    },

    approveNewProject: async (mailId, projectName, description) => {
      try {
        const project = await classifyApi.approveNewProject(
          mailId,
          projectName,
          description,
        );
        useProjectStore.getState().addProject(project);
        set({ pendingProposal: null });
        await classifyNext();
      } catch (e) {
        useErrorStore.getState().addError(errorMessage(e));
      }
    },

    rejectClassification: async (mailId) => {
      try {
        await classifyApi.rejectClassification(mailId);
      } catch (e) {
        useErrorStore.getState().addError(errorMessage(e));
      }
      set({ pendingProposal: null });
      await classifyNext();
    },
  };
});
