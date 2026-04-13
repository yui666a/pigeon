import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Mail, Thread } from "../types/mail";

interface MailState {
  threads: Thread[];
  selectedThread: Thread | null;
  selectedMail: Mail | null;
  syncing: boolean;
  error: string | null;
  fetchThreads: (accountId: string, folder: string) => Promise<void>;
  syncAccount: (accountId: string) => Promise<number>;
  selectThread: (thread: Thread | null) => void;
  selectMail: (mail: Mail | null) => void;
}

export const useMailStore = create<MailState>((set) => ({
  threads: [],
  selectedThread: null,
  selectedMail: null,
  syncing: false,
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
      return 0;
    }
  },

  selectThread: (thread) => set({ selectedThread: thread, selectedMail: null }),
  selectMail: (mail) => set({ selectedMail: mail }),
}));
