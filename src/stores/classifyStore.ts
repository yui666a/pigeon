import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useErrorStore } from "./errorStore";
import type {
  ClassifyResponse,
  ClassifyProgress,
  ClassifySummary,
} from "../types/classifier";

interface ClassifyState {
  classifying: boolean;
  classifyingAccountId: string | null;
  progress: { current: number; total: number } | null;
  results: ClassifyResponse[];
  summary: ClassifySummary | null;
  error: string | null;
  classifyMail: (mailId: string) => Promise<void>;
  classifyAll: (accountId: string) => Promise<void>;
  cancelClassification: () => Promise<void>;
  approveClassification: (mailId: string, projectId: string) => Promise<void>;
  approveNewProject: (
    mailId: string,
    projectName: string,
    description?: string,
  ) => Promise<void>;
  rejectClassification: (mailId: string) => Promise<void>;
  initClassifyListeners: () => Promise<() => void>;
}

export const useClassifyStore = create<ClassifyState>((set, get) => ({
  classifying: false,
  classifyingAccountId: null,
  progress: null,
  results: [],
  summary: null,
  error: null,

  classifyMail: async (mailId) => {
    set({ classifying: true, error: null });
    try {
      const result = await invoke<ClassifyResponse>("classify_mail", {
        mailId,
      });
      set({
        results: [...get().results, result],
        classifying: false,
      });
    } catch (e) {
      set({ error: String(e), classifying: false });
      useErrorStore.getState().addError(String(e));
    }
  },

  classifyAll: async (accountId) => {
    set({ classifying: true, classifyingAccountId: accountId, progress: null, results: [], summary: null, error: null });
    try {
      await invoke("classify_unassigned", { accountId });
    } catch (e) {
      set({ error: String(e), classifying: false, classifyingAccountId: null, progress: null });
      useErrorStore.getState().addError(String(e));
    }
  },

  cancelClassification: async () => {
    try {
      await invoke("cancel_classification");
      set({ classifying: false, progress: null });
    } catch (e) {
      set({ error: String(e) });
      useErrorStore.getState().addError(String(e));
    }
  },

  approveClassification: async (mailId, projectId) => {
    try {
      await invoke("approve_classification", { mailId, projectId });
      set({
        results: get().results.filter((r) => r.mail_id !== mailId),
      });
    } catch (e) {
      set({ error: String(e) });
      useErrorStore.getState().addError(String(e));
    }
  },

  approveNewProject: async (mailId, projectName, description) => {
    try {
      await invoke("approve_new_project", {
        mailId,
        projectName,
        description: description ?? null,
      });
      set({
        results: get().results.filter((r) => r.mail_id !== mailId),
      });
    } catch (e) {
      set({ error: String(e) });
      useErrorStore.getState().addError(String(e));
    }
  },

  rejectClassification: async (mailId) => {
    try {
      await invoke("reject_classification", { mailId });
      set({
        results: get().results.filter((r) => r.mail_id !== mailId),
      });
    } catch (e) {
      set({ error: String(e) });
      useErrorStore.getState().addError(String(e));
    }
  },

  initClassifyListeners: async () => {
    const unlistenProgress = await listen<ClassifyProgress>(
      "classify-progress",
      (event) => {
        const payload = event.payload;
        if (payload.result) {
          set({
            progress: { current: payload.current, total: payload.total },
            results: [...get().results, payload.result],
          });
        } else {
          set({
            progress: { current: payload.current, total: payload.total },
          });
        }
      },
    );

    const unlistenComplete = await listen<ClassifySummary>(
      "classify-complete",
      (event) => {
        set({
          summary: event.payload,
          classifying: false,
          classifyingAccountId: null,
          progress: null,
        });
      },
    );

    return () => {
      unlistenProgress();
      unlistenComplete();
    };
  },
}));
