import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Mail, Thread } from "../types/mail";
import { useErrorStore } from "./errorStore";

interface MailState {
  threads: Thread[];
  selectedThread: Thread | null;
  selectedMail: Mail | null;
  syncing: boolean;
  unclassifiedMails: Mail[];
  error: string | null;
  fetchThreads: (accountId: string, folder: string) => Promise<void>;
  syncAccount: (accountId: string) => Promise<number>;
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
    set({ syncing: true, error: null });
    try {
      const count = await invoke<number>("sync_account", { accountId });
      set({ syncing: false });
      return count;
    } catch (e) {
      set({ error: String(e), syncing: false });
      useErrorStore.getState().addError(String(e));
      return 0;
    }
  },

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
