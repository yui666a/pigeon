import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Mail, Thread } from "../types/mail";
import { useErrorStore } from "./errorStore";

interface SyncAccountResult {
  count: number;
  reauth_required: boolean;
}

interface MailState {
  threads: Thread[];
  selectedThread: Thread | null;
  selectedMail: Mail | null;
  syncing: boolean;
  needsReauthAccountId: string | null;
  unclassifiedMails: Mail[];
  error: string | null;
  fetchThreads: (accountId: string, folder: string) => Promise<void>;
  syncAccount: (accountId: string) => Promise<number>;
  clearNeedsReauth: (accountId?: string) => void;
  setThreads: (threads: Thread[]) => void;
  selectThread: (thread: Thread | null) => void;
  selectMail: (mail: Mail | null) => void;
  fetchUnclassified: (accountId: string) => Promise<void>;
  moveMail: (mailId: string, projectId: string) => Promise<void>;
  removeUnclassifiedMail: (mailId: string) => void;
}

export const useMailStore = create<MailState>((set, get) => ({
  threads: [],
  selectedThread: null,
  selectedMail: null,
  syncing: false,
  needsReauthAccountId: null,
  unclassifiedMails: [],
  error: null,

  fetchThreads: async (accountId, folder) => {
    try {
      const threads = await invoke<Thread[]>("get_threads", {
        accountId,
        folder,
      });
      set({ threads });
    } catch (e) {
      set({ error: String(e) });
      useErrorStore.getState().addError(String(e));
    }
  },

  syncAccount: async (accountId) => {
    set((state) => ({
      syncing: true,
      error: null,
      needsReauthAccountId:
        state.needsReauthAccountId === accountId ? null : state.needsReauthAccountId,
    }));
    try {
      const result = await invoke<SyncAccountResult>("sync_account", { accountId });
      if (result.reauth_required) {
        set({
          syncing: false,
          error: null,
          needsReauthAccountId: accountId,
        });
        return 0;
      }
      set({ syncing: false });
      return result.count;
    } catch (e) {
      const errorMsg = String(e);
      set((state) => ({
        error: errorMsg,
        syncing: false,
        needsReauthAccountId:
          state.needsReauthAccountId === accountId ? null : state.needsReauthAccountId,
      }));
      useErrorStore.getState().addError(errorMsg);
      return 0;
    }
  },

  clearNeedsReauth: (accountId) =>
    set((state) => {
      if (!accountId || state.needsReauthAccountId === accountId) {
        return { needsReauthAccountId: null };
      }
      return {};
    }),

  setThreads: (threads) => set({ threads }),
  selectThread: (thread) => set({ selectedThread: thread, selectedMail: null }),
  selectMail: (mail) => set({ selectedMail: mail }),

  fetchUnclassified: async (accountId) => {
    try {
      const mails = await invoke<Mail[]>("get_unclassified_mails", {
        accountId,
      });
      set({ unclassifiedMails: mails });
    } catch (e) {
      set({ error: String(e) });
      useErrorStore.getState().addError(String(e));
    }
  },

  moveMail: async (mailId, projectId) => {
    try {
      await invoke("move_mail", { mailId, projectId });
      set({
        unclassifiedMails: get().unclassifiedMails.filter((m) => m.id !== mailId),
      });
    } catch (e) {
      set({ error: String(e) });
      useErrorStore.getState().addError(String(e));
    }
  },

  removeUnclassifiedMail: (mailId) => {
    set({
      unclassifiedMails: get().unclassifiedMails.filter((m) => m.id !== mailId),
    });
  },
}));
