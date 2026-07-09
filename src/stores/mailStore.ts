import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Mail, Thread } from "../types/mail";
import { useErrorStore } from "./errorStore";

interface SyncProgress {
  account_id: string;
  done: number;
  total: number;
}

interface MailState {
  threads: Thread[];
  selectedThread: Thread | null;
  selectedMail: Mail | null;
  syncing: boolean;
  needsReauth: boolean;
  unclassifiedMails: Mail[];
  error: string | null;
  syncProgress: SyncProgress | null;
  fetchThreads: (accountId: string, folder: string) => Promise<void>;
  syncAccount: (accountId: string) => Promise<number>;
  setThreads: (threads: Thread[]) => void;
  selectThread: (thread: Thread | null) => void;
  selectMail: (mail: Mail | null) => void;
  fetchUnclassified: (accountId: string) => Promise<void>;
  moveMail: (mailId: string, projectId: string) => Promise<void>;
  removeUnclassifiedMail: (mailId: string) => void;
  initSyncListener: () => Promise<() => void>;
}

export const useMailStore = create<MailState>((set, get) => ({
  threads: [],
  selectedThread: null,
  selectedMail: null,
  syncing: false,
  needsReauth: false,
  unclassifiedMails: [],
  error: null,
  syncProgress: null,

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
    set({ syncing: true, error: null, needsReauth: false });
    try {
      const count = await invoke<number>("sync_account", { accountId });
      set({ syncing: false, syncProgress: null });
      return count;
    } catch (e) {
      const errorMsg = String(e);
      const isReauth = errorMsg.includes("Reauth required");
      set({ error: errorMsg, syncing: false, needsReauth: isReauth, syncProgress: null });
      if (!isReauth) {
        useErrorStore.getState().addError(errorMsg);
      }
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

  initSyncListener: async () => {
    const unlisten = await listen<SyncProgress>("sync-progress", (event) => {
      const p = event.payload;
      set({ syncProgress: p });
      // 一覧への順次反映は500件ごと（=5バッチに1回）と完了時のみ。
      // 毎バッチのDB再読込を避ける
      if (p.done % 500 === 0 || p.done === p.total) {
        void get().fetchThreads(p.account_id, "INBOX");
        void get().fetchUnclassified(p.account_id);
      }
    });
    return unlisten;
  },
}));
