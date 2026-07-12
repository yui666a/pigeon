import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Mail, Thread, UnreadCounts } from "../types/mail";
import { useErrorStore } from "./errorStore";
import { useAccountStore } from "./accountStore";
import { useUiStore } from "./uiStore";

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
  unreadCounts: UnreadCounts;
  fetchThreads: (accountId: string, folder: string) => Promise<void>;
  syncAccount: (accountId: string) => Promise<number>;
  setThreads: (threads: Thread[]) => void;
  selectThread: (thread: Thread | null) => void;
  selectMail: (mail: Mail | null) => void;
  markMailRead: (mail: Mail) => void;
  fetchUnreadCounts: (accountId: string) => Promise<void>;
  fetchUnclassified: (accountId: string) => Promise<void>;
  moveMail: (mailId: string, projectId: string) => Promise<void>;
  removeUnclassifiedMail: (mailId: string) => void;
  initSyncListener: () => Promise<() => void>;
}

function markReadInMails(mails: Mail[], mailId: string): Mail[] {
  return mails.map((m) => (m.id === mailId ? { ...m, is_read: true } : m));
}

function markReadInThread(thread: Thread, mailId: string): Thread {
  if (!thread.mails.some((m) => m.id === mailId)) return thread;
  return { ...thread, mails: markReadInMails(thread.mails, mailId) };
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
  unreadCounts: { by_project: {}, unclassified: 0 },

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
    // 多重実行ガード（バックエンドにもアカウント単位ロックがあり、これは
    // 画面遷移や開発モードの二重effectで無駄なinvokeを出さないための前段）
    if (get().syncing) return 0;
    set({ syncing: true, error: null, needsReauth: false });
    try {
      const count = await invoke<number>("sync_account", { accountId });
      set({ syncing: false, syncProgress: null });
      // 同期でフラグ再同期（他クライアントの既読変更）が反映されるため取り直す
      void get().fetchUnreadCounts(accountId);
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

  selectThread: (thread) => {
    set({ selectedThread: thread, selectedMail: null });
    // スレッド選択時は末尾（最新）のメールが本文表示される
    const displayed = thread?.mails[thread.mails.length - 1];
    if (displayed && !displayed.is_read) {
      get().markMailRead(displayed);
    }
  },

  selectMail: (mail) => {
    set({ selectedMail: mail });
    if (mail && !mail.is_read) {
      get().markMailRead(mail);
    }
  },

  markMailRead: (mail) => {
    if (mail.is_read) return;
    // ローカルは即時確定。サーバーへの \Seen 反映はバックエンドが
    // バックグラウンドでベストエフォート実行する（失敗しても既読は維持）
    set((state) => ({
      threads: state.threads.map((t) => markReadInThread(t, mail.id)),
      selectedThread: state.selectedThread
        ? markReadInThread(state.selectedThread, mail.id)
        : state.selectedThread,
      selectedMail:
        state.selectedMail?.id === mail.id
          ? { ...state.selectedMail, is_read: true }
          : state.selectedMail,
      unclassifiedMails: markReadInMails(state.unclassifiedMails, mail.id),
    }));
    invoke("mark_read", { accountId: mail.account_id, mailId: mail.id })
      .then(() => get().fetchUnreadCounts(mail.account_id))
      .catch((e) => {
        console.error("mark_read failed:", e);
      });
  },

  fetchUnreadCounts: async (accountId) => {
    try {
      const counts = await invoke<UnreadCounts>("get_unread_counts", {
        accountId,
      });
      set({
        unreadCounts: {
          by_project: counts?.by_project ?? {},
          unclassified: counts?.unclassified ?? 0,
        },
      });
    } catch (e) {
      // 未読バッジは補助情報のためトーストは出さない（前回値を維持）
      console.error("get_unread_counts failed:", e);
    }
  },

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
        // 同期中アカウントを表示している場合のみ一覧へ順次反映する。
        // 別アカウント・案件ビュー・検索を見ているときに INBOX で上書きしない
        const selectedAccountId = useAccountStore.getState().selectedAccountId;
        if (selectedAccountId !== p.account_id) return;
        if (useUiStore.getState().viewMode === "threads") {
          void get().fetchThreads(p.account_id, "INBOX");
        }
        void get().fetchUnclassified(p.account_id);
      }
    });
    return unlisten;
  },
}));
