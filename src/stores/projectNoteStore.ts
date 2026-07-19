import { create } from "zustand";
import { projectNoteApi } from "../api/projectNoteApi";
import { errorMessage } from "../api/errors";
import { useErrorStore } from "./errorStore";
import type { AiHistoryEntry, ProjectNote } from "../types/projectNote";

function emptyNote(projectId: string): ProjectNote {
  return {
    project_id: projectId,
    user_md: "",
    ai_md: null,
    ai_edited: false,
    ai_generated_at: null,
    updated_at: null,
  };
}

interface ProjectNoteState {
  note: ProjectNote | null;
  history: AiHistoryEntry[];
  loading: boolean;
  generating: boolean;
  load: (projectId: string) => Promise<void>;
  saveUser: (projectId: string, userMd: string) => Promise<void>;
  saveAi: (projectId: string, aiMd: string) => Promise<void>;
  generate: (projectId: string) => Promise<void>;
  loadHistory: (projectId: string) => Promise<void>;
  restore: (projectId: string, historyId: string) => Promise<void>;
}

export const useProjectNoteStore = create<ProjectNoteState>((set, get) => ({
  note: null,
  history: [],
  loading: false,
  generating: false,

  load: async (projectId) => {
    set({ loading: true });
    try {
      const note = await projectNoteApi.fetchNote(projectId);
      set({ note: note ?? emptyNote(projectId), loading: false });
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  saveUser: async (projectId, userMd) => {
    try {
      await projectNoteApi.saveUserNote(projectId, userMd);
      const cur = get().note ?? emptyNote(projectId);
      set({ note: { ...cur, user_md: userMd } });
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  saveAi: async (projectId, aiMd) => {
    try {
      await projectNoteApi.saveAiNote(projectId, aiMd);
      const cur = get().note ?? emptyNote(projectId);
      set({ note: { ...cur, ai_md: aiMd, ai_edited: true } });
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  generate: async (projectId) => {
    set({ generating: true });
    try {
      const out = await projectNoteApi.generateAiNote(projectId);
      const cur = get().note ?? emptyNote(projectId);
      set({
        note: { ...cur, ai_md: out.ai_md, ai_edited: false },
        generating: false,
      });
    } catch (e) {
      // 生成失敗時は既存の ai_md を保持する（LLM呼び出し失敗でユーザーの要約を消さない）
      set({ generating: false });
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  loadHistory: async (projectId) => {
    try {
      const history = await projectNoteApi.fetchAiHistory(projectId);
      set({ history });
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  restore: async (projectId, historyId) => {
    try {
      await projectNoteApi.restoreAiNote(historyId);
      await get().load(projectId);
      await get().loadHistory(projectId);
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },
}));
