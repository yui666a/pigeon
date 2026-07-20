import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import type { BulkResult, Mail, Thread, ThreadPage, UnreadCounts } from "../types/mail";
import type { NewMailEvent, SyncProgress } from "../types/events";
import { mailApi } from "../api/mailApi";
import { errorMessage, isReauthError } from "../api/errors";
import { useErrorStore } from "./errorStore";
import { useAccountStore } from "./accountStore";
import { useProjectStore } from "./projectStore";
import { useUiStore } from "./uiStore";
import { notifyNewMail } from "../utils/notifyNewMail";
import { INBOX_FOLDER } from "../constants/folders";
import { THREAD_PAGE_SIZE } from "../constants/paging";

interface MailState {
  threads: Thread[];
  selectedThread: Thread | null;
  selectedMail: Mail | null;
  syncing: boolean;
  needsReauth: boolean;
  unclassifiedMails: Mail[];
  /** 未分類メールのスレッド表示用（unclassifiedMails と同一内容のスレッド版） */
  unclassifiedThreads: Thread[];
  syncProgress: SyncProgress | null;
  unreadCounts: UnreadCounts;
  backfilling: boolean;
  backfillProgress: SyncProgress | null;
  /** account_id -> これ以上サーバーに古いメールが無いか（ボタン無効化の判定用） */
  backfillExhausted: Record<string, boolean>;
  /** サーバ側にまだ後続スレッドがあるか（一覧の「もっと見る」の表示判定） */
  hasMoreThreads: boolean;
  /** 未分類一覧にまだ後続スレッドがあるか */
  hasMoreUnclassified: boolean;
  fetchThreads: (accountId: string, folder: string) => Promise<void>;
  fetchThreadsByProject: (projectId: string) => Promise<void>;
  /** 現在の一覧の続きを1ページ分追加取得する（案件ビュー / INBOX を自動判別） */
  fetchMoreThreads: () => Promise<void>;
  /** 未分類一覧の続きを1ページ分追加取得する */
  fetchMoreUnclassified: (accountId: string) => Promise<void>;
  syncAccount: (accountId: string) => Promise<number>;
  backfillAccount: (accountId: string, limit: number) => Promise<void>;
  setThreads: (threads: Thread[]) => void;
  selectThread: (thread: Thread | null) => void;
  selectMail: (mail: Mail | null) => void;
  markMailRead: (mail: Mail) => void;
  toggleFlagged: (mail: Mail) => Promise<void>;
  markMailUnread: (mail: Mail) => Promise<void>;
  deleteMail: (mail: Mail) => Promise<void>;
  archiveMail: (mail: Mail) => Promise<void>;
  unarchiveMail: (mail: Mail) => Promise<void>;
  fetchUnreadCounts: (accountId: string) => Promise<void>;
  fetchUnclassified: (accountId: string) => Promise<void>;
  removeUnclassifiedMail: (mailId: string) => void;
  /** AI分類の確定を表示へ反映する。案件が変わった場合は現在の案件ビューから取り除く */
  applyAssignmentApproved: (mailId: string, projectId: string) => void;
  initSyncListener: () => Promise<() => void>;
  initNewMailListener: () => Promise<() => void>;
  initBackfillListener: () => Promise<() => void>;
  bulkDeleteMails: (accountId: string, mailIds: string[]) => Promise<BulkResult | null>;
  bulkArchiveMails: (accountId: string, mailIds: string[]) => Promise<BulkResult | null>;
  bulkMoveMails: (mailIds: string[], projectId: string) => Promise<BulkResult | null>;
}

function markReadInMails(mails: Mail[], mailId: string): Mail[] {
  return mails.map((m) => (m.id === mailId ? { ...m, is_read: true } : m));
}

function markReadInThread(thread: Thread, mailId: string): Thread {
  if (!thread.mails.some((m) => m.id === mailId)) return thread;
  return { ...thread, mails: markReadInMails(thread.mails, mailId) };
}

/** スレッドからメールを除去する。空になったら null（スレッドごと除去） */
function removeMailFromThread(thread: Thread, mailId: string): Thread | null {
  if (!thread.mails.some((m) => m.id === mailId)) return thread;
  const mails = thread.mails.filter((m) => m.id !== mailId);
  if (mails.length === 0) return null;
  return {
    ...thread,
    mails,
    mail_count: mails.length,
    last_date: mails[mails.length - 1].date,
  };
}

function setFlaggedInMails(mails: Mail[], mailId: string, flagged: boolean): Mail[] {
  return mails.map((m) => (m.id === mailId ? { ...m, is_flagged: flagged } : m));
}

function setFlaggedInThread(thread: Thread, mailId: string, flagged: boolean): Thread {
  if (!thread.mails.some((m) => m.id === mailId)) return thread;
  return { ...thread, mails: setFlaggedInMails(thread.mails, mailId, flagged) };
}

/** スター/フラグを表示用の全状態へ反映する（toggleFlagged の楽観更新・失敗時のロールバック共用） */
function setFlaggedInState(
  state: MailState,
  mailId: string,
  flagged: boolean,
): Partial<MailState> {
  return {
    threads: state.threads.map((t) => setFlaggedInThread(t, mailId, flagged)),
    selectedThread: state.selectedThread
      ? setFlaggedInThread(state.selectedThread, mailId, flagged)
      : null,
    selectedMail:
      state.selectedMail?.id === mailId
        ? { ...state.selectedMail, is_flagged: flagged }
        : state.selectedMail,
    unclassifiedMails: setFlaggedInMails(state.unclassifiedMails, mailId, flagged),
    unclassifiedThreads: state.unclassifiedThreads.map((t) =>
      setFlaggedInThread(t, mailId, flagged),
    ),
  };
}

function markUnreadInMails(mails: Mail[], mailId: string): Mail[] {
  return mails.map((m) => (m.id === mailId ? { ...m, is_read: false } : m));
}

function markUnreadInThread(thread: Thread, mailId: string): Thread {
  if (!thread.mails.some((m) => m.id === mailId)) return thread;
  return { ...thread, mails: markUnreadInMails(thread.mails, mailId) };
}

function setFolderInMails(mails: Mail[], mailId: string, folder: string): Mail[] {
  return mails.map((m) => (m.id === mailId ? { ...m, folder } : m));
}

function setFolderInThread(thread: Thread, mailId: string, folder: string): Thread {
  if (!thread.mails.some((m) => m.id === mailId)) return thread;
  return { ...thread, mails: setFolderInMails(thread.mails, mailId, folder) };
}

/** アーカイブ解除成功後に、表示用の全状態で該当メールの folder を更新する。
 * 除去はしない: アーカイブ済みメールが見えるのは案件ビュー・検索であり、
 * 解除後も同じ場所に表示され続けるのが自然なため（設計書「アーカイブ解除」） */
function setFolderInState(
  state: MailState,
  mailId: string,
  folder: string,
): Partial<MailState> {
  return {
    threads: state.threads.map((t) => setFolderInThread(t, mailId, folder)),
    selectedThread: state.selectedThread
      ? setFolderInThread(state.selectedThread, mailId, folder)
      : null,
    selectedMail:
      state.selectedMail?.id === mailId
        ? { ...state.selectedMail, folder }
        : state.selectedMail,
    unclassifiedMails: setFolderInMails(state.unclassifiedMails, mailId, folder),
  };
}

/** 削除・アーカイブ成功後に、表示用の全状態から該当メールを取り除く */
function setAssignedByInMails(mails: Mail[], mailId: string, assignedBy: string): Mail[] {
  return mails.map((m) => (m.id === mailId ? { ...m, assigned_by: assignedBy } : m));
}

function setAssignedByInThread(thread: Thread, mailId: string, assignedBy: string): Thread {
  if (!thread.mails.some((m) => m.id === mailId)) return thread;
  return { ...thread, mails: setAssignedByInMails(thread.mails, mailId, assignedBy) };
}

/** 分類の確定（assigned_by='user'）を表示用の全状態へ反映する */
function setAssignedByInState(
  state: MailState,
  mailId: string,
  assignedBy: string,
): Partial<MailState> {
  return {
    threads: state.threads.map((t) => setAssignedByInThread(t, mailId, assignedBy)),
    selectedThread: state.selectedThread
      ? setAssignedByInThread(state.selectedThread, mailId, assignedBy)
      : null,
    selectedMail:
      state.selectedMail?.id === mailId
        ? { ...state.selectedMail, assigned_by: assignedBy }
        : state.selectedMail,
    unclassifiedMails: setAssignedByInMails(state.unclassifiedMails, mailId, assignedBy),
    unclassifiedThreads: state.unclassifiedThreads.map((t) =>
      setAssignedByInThread(t, mailId, assignedBy),
    ),
  };
}

function removeMailFromState(state: MailState, mailId: string): Partial<MailState> {
  return {
    threads: state.threads
      .map((t) => removeMailFromThread(t, mailId))
      .filter((t): t is Thread => t !== null),
    selectedThread: state.selectedThread
      ? removeMailFromThread(state.selectedThread, mailId)
      : null,
    selectedMail: state.selectedMail?.id === mailId ? null : state.selectedMail,
    unclassifiedMails: state.unclassifiedMails.filter((m) => m.id !== mailId),
    unclassifiedThreads: state.unclassifiedThreads
      .map((t) => removeMailFromThread(t, mailId))
      .filter((t): t is Thread => t !== null),
  };
}

/**
 * ThreadPage を安全に取り出す。バックエンドは常に { threads, has_more } を
 * 返すが、想定外の形が来ても一覧を undefined にして描画を壊さない。
 */
function toPage(page: ThreadPage | undefined | null): ThreadPage {
  return {
    threads: page?.threads ?? [],
    has_more: page?.has_more ?? false,
  };
}

export const useMailStore = create<MailState>((set, get) => ({
  threads: [],
  selectedThread: null,
  selectedMail: null,
  syncing: false,
  needsReauth: false,
  unclassifiedMails: [],
  unclassifiedThreads: [],
  syncProgress: null,
  unreadCounts: { by_project: {}, unclassified: 0 },
  backfilling: false,
  backfillProgress: null,
  backfillExhausted: {},
  hasMoreThreads: false,
  hasMoreUnclassified: false,

  // 一覧の先頭ページを取得する（再取得のたびに先頭へ戻す）
  fetchThreads: async (accountId, folder) => {
    try {
      const page = toPage(await mailApi.fetchThreads(accountId, folder, THREAD_PAGE_SIZE, 0));
      set({ threads: page.threads, hasMoreThreads: page.has_more });
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  // 案件ビューのスレッド一覧を取得する。取得中に別案件へ切り替わった場合は
  // 反映しない（sync-progress リスナーの「表示中のみ反映」方針と同じ）
  fetchThreadsByProject: async (projectId) => {
    try {
      const page = toPage(await mailApi.fetchThreadsByProject(projectId, THREAD_PAGE_SIZE, 0));
      if (useProjectStore.getState().selectedProjectId !== projectId) return;
      set({ threads: page.threads, hasMoreThreads: page.has_more });
    } catch (e) {
      // 失敗時に前のビューの一覧を残すと紛らわしいためクリアする
      if (useProjectStore.getState().selectedProjectId === projectId) {
        set({ threads: [], hasMoreThreads: false });
      }
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  // 続きのページを取得して現在の一覧に追記する。offset は「今表示している
  // スレッド数」＝次に読むべき位置。案件ビューか INBOX かは選択状態で判別する
  fetchMoreThreads: async () => {
    const { threads, hasMoreThreads } = get();
    if (!hasMoreThreads) return;
    const offset = threads.length;
    const projectId = useProjectStore.getState().selectedProjectId;
    const accountId = useAccountStore.getState().selectedAccountId;
    try {
      const raw = projectId
        ? await mailApi.fetchThreadsByProject(projectId, THREAD_PAGE_SIZE, offset)
        : accountId
          ? await mailApi.fetchThreads(accountId, INBOX_FOLDER, THREAD_PAGE_SIZE, offset)
          : null;
      if (!raw) return;
      const page = toPage(raw);
      // 取得中に一覧が差し替わっていたら追記しない（先頭ページの取り直しと競合させない）
      if (get().threads.length !== offset) return;
      set((state) => ({
        threads: [...state.threads, ...page.threads],
        hasMoreThreads: page.has_more,
      }));
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  fetchMoreUnclassified: async (accountId) => {
    const { unclassifiedThreads, hasMoreUnclassified } = get();
    if (!hasMoreUnclassified) return;
    const offset = unclassifiedThreads.length;
    try {
      const page = toPage(
        await mailApi.fetchUnclassifiedThreads(accountId, THREAD_PAGE_SIZE, offset),
      );
      if (get().unclassifiedThreads.length !== offset) return;
      set((state) => ({
        unclassifiedThreads: [...state.unclassifiedThreads, ...page.threads],
        unclassifiedMails: [...state.unclassifiedMails, ...page.threads.flatMap((t) => t.mails)],
        hasMoreUnclassified: page.has_more,
      }));
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  syncAccount: async (accountId) => {
    // 多重実行ガード（バックエンドにもアカウント単位ロックがあり、これは
    // 画面遷移や開発モードの二重effectで無駄なinvokeを出さないための前段）
    if (get().syncing) return 0;
    set({ syncing: true, needsReauth: false });
    try {
      const count = await mailApi.syncAccount(accountId);
      set({ syncing: false, syncProgress: null });
      // 同期でフラグ再同期（他クライアントの既読変更）が反映されるため取り直す
      void get().fetchUnreadCounts(accountId);
      return count;
    } catch (e) {
      const isReauth = isReauthError(e);
      set({ syncing: false, needsReauth: isReauth, syncProgress: null });
      if (!isReauth) {
        useErrorStore.getState().addError(errorMessage(e));
      }
      return 0;
    }
  },

  // ローカル最古メールより古いメールを、新しい→古いの順に limit 件まで遡って取得する
  // （バックログ項目8）。バックエンドの SyncLocks を通常同期と共有しているため多重実行は
  // 防がれるが、無駄な invoke を出さない前段ガードとして backfilling フラグも見る
  // （syncAccount と同型）
  backfillAccount: async (accountId, limit) => {
    if (get().backfilling) return;
    set({ backfilling: true });
    try {
      const outcome = await mailApi.backfillAccount(accountId, limit);
      set((state) => ({
        backfilling: false,
        backfillProgress: null,
        backfillExhausted: { ...state.backfillExhausted, [accountId]: outcome.exhausted },
      }));
      // 表示中アカウント・ビューのみ再取得する（syncAccount の sync-progress
      // リスナーと同じ「別アカウント表示中は上書きしない」方針）
      const selectedAccountId = useAccountStore.getState().selectedAccountId;
      if (selectedAccountId === accountId) {
        if (useUiStore.getState().viewMode === "threads") {
          void get().fetchThreads(accountId, INBOX_FOLDER);
        }
        void get().fetchUnclassified(accountId);
      }
    } catch (e) {
      set({ backfilling: false, backfillProgress: null });
      useErrorStore.getState().addError(errorMessage(e));
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
      unclassifiedThreads: state.unclassifiedThreads.map((t) =>
        markReadInThread(t, mail.id),
      ),
    }));
    mailApi
      .markRead(mail.account_id, mail.id)
      .then(() => get().fetchUnreadCounts(mail.account_id))
      .catch((e) => {
        console.error("mark_read failed:", e);
      });
  },

  // スター/フラグは既読と同様に楽観更新するが、既読と違い頻繁にトグルされ
  // 誤操作の是正もしやすいため、失敗時はロールバックしてエラー表示する
  // （既読の「サーバー失敗はログのみ」より一段階ユーザーに見える形にする）
  toggleFlagged: async (mail) => {
    const next = !mail.is_flagged;
    set((state) => setFlaggedInState(state, mail.id, next));
    try {
      await mailApi.setFlagged(mail.account_id, mail.id, next);
    } catch (e) {
      set((state) => setFlaggedInState(state, mail.id, !next));
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  // 未読に戻す。mark_read の逆で DB は即時更新・サーバー反映はベストエフォート
  // だが、未読化した本文を表示したままだと selectMail の自動既読化で即座に
  // 既読へ戻ってしまうため、成功時は選択を解除する（設計書「自動既読化との干渉回避」）
  markMailUnread: async (mail) => {
    set((state) => ({
      threads: state.threads.map((t) => markUnreadInThread(t, mail.id)),
      selectedThread: state.selectedThread
        ? markUnreadInThread(state.selectedThread, mail.id)
        : state.selectedThread,
      unclassifiedMails: markUnreadInMails(state.unclassifiedMails, mail.id),
      unclassifiedThreads: state.unclassifiedThreads.map((t) =>
        markUnreadInThread(t, mail.id),
      ),
    }));
    try {
      await mailApi.markUnread(mail.account_id, mail.id);
      set({ selectedMail: null });
      void get().fetchUnreadCounts(mail.account_id);
    } catch (e) {
      // ロールバック: 既読状態に戻す
      set((state) => ({
        threads: state.threads.map((t) => markReadInThread(t, mail.id)),
        selectedThread: state.selectedThread
          ? markReadInThread(state.selectedThread, mail.id)
          : state.selectedThread,
        unclassifiedMails: markReadInMails(state.unclassifiedMails, mail.id),
        unclassifiedThreads: state.unclassifiedThreads.map((t) =>
          markReadInThread(t, mail.id),
        ),
      }));
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  // 削除は破壊的操作のため楽観更新しない: サーバー反映（invoke）が成功した
  // 場合のみローカル状態から除去する。失敗時はエラー表示のみで状態は変えない
  deleteMail: async (mail) => {
    try {
      await mailApi.deleteMail(mail.account_id, mail.id);
      set((state) => removeMailFromState(state, mail.id));
      void get().fetchUnreadCounts(mail.account_id);
      useErrorStore.getState().addSuccess("削除しました");
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  archiveMail: async (mail) => {
    try {
      await mailApi.archiveMail(mail.account_id, mail.id);
      set((state) => removeMailFromState(state, mail.id));
      void get().fetchUnreadCounts(mail.account_id);
      useErrorStore.getState().addSuccess("アーカイブしました");
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  // アーカイブ解除。v1 はローカルの folder 更新のみ（サーバー反映はバック
  // エンドが行わない: UID を追跡できないため。設計書「アーカイブ解除」参照）。
  // 成功時のみ folder を 'INBOX' へ更新する（除去はしない）
  unarchiveMail: async (mail) => {
    try {
      await mailApi.unarchiveMail(mail.account_id, mail.id);
      set((state) => setFolderInState(state, mail.id, INBOX_FOLDER));
      void get().fetchUnreadCounts(mail.account_id);
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  fetchUnreadCounts: async (accountId) => {
    try {
      const counts = await mailApi.fetchUnreadCounts(accountId);
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
      // スレッド単位で取得し、メール一覧はフラット化して両方の状態を一致させる
      const page = toPage(
        await mailApi.fetchUnclassifiedThreads(accountId, THREAD_PAGE_SIZE, 0),
      );
      set({
        unclassifiedThreads: page.threads,
        unclassifiedMails: page.threads.flatMap((t) => t.mails),
        hasMoreUnclassified: page.has_more,
      });
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  removeUnclassifiedMail: (mailId) => {
    set((state) => ({
      unclassifiedMails: state.unclassifiedMails.filter((m) => m.id !== mailId),
      unclassifiedThreads: state.unclassifiedThreads
        .map((t) => removeMailFromThread(t, mailId))
        .filter((t): t is Thread => t !== null),
    }));
  },

  applyAssignmentApproved: (mailId, projectId) => {
    const viewing = useProjectStore.getState().selectedProjectId;
    // 別案件へ修正した場合、今見ている案件ビューにはもう属さないので取り除く
    // （一括移動と同じ楽観更新の挙動に揃える）
    if (viewing && viewing !== projectId) {
      set((state) => removeMailFromState(state, mailId));
      return;
    }
    set((state) => setAssignedByInState(state, mailId, "user"));
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
          void get().fetchThreads(p.account_id, INBOX_FOLDER);
        }
        void get().fetchUnclassified(p.account_id);
      }
    });
    return unlisten;
  },

  initBackfillListener: async () => {
    const unlisten = await listen<SyncProgress>("backfill-progress", (event) => {
      set({ backfillProgress: event.payload });
    });
    return unlisten;
  },

  initNewMailListener: async () => {
    // バックエンドの IMAP IDLE 監視が新着を検知したら、既存の同期経路で取り込む。
    // 表示中アカウントと無関係に同期してよい（一覧への反映可否は sync-progress
    // リスナーが判断する）。多重実行は syncing フラグと SyncLocks が抑止する
    const unlisten = await listen<NewMailEvent>("new-mail-detected", (event) => {
      void get()
        .syncAccount(event.payload.account_id)
        .then((count) => {
          // 実際に取り込まれた件数を条件にする（IDLE の誤検知や
          // 同期中ガード・エラー時の count=0 では空通知を出さない）
          if (count > 0) void notifyNewMail(count, event.payload.account_id);
        });
    });
    return unlisten;
  },

  // 一括操作。呼び出し元（ThreadList / UnclassifiedList）が結果を見て
  // 一覧再読み込みと選択解除を行う（設計書 2026-07-13-bulk-actions-design.md）
  bulkDeleteMails: async (accountId, mailIds) => {
    try {
      const result = await mailApi.bulkDeleteMails(accountId, mailIds);
      reportBulkResult(result, "削除");
      void get().fetchUnreadCounts(accountId);
      return result;
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
      return null;
    }
  },

  bulkArchiveMails: async (accountId, mailIds) => {
    try {
      const result = await mailApi.bulkArchiveMails(accountId, mailIds);
      reportBulkResult(result, "アーカイブ");
      void get().fetchUnreadCounts(accountId);
      return result;
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
      return null;
    }
  },

  bulkMoveMails: async (mailIds, projectId) => {
    try {
      const result = await mailApi.bulkMoveMails(mailIds, projectId);
      // 移動が確定したメールは表示中のリスト（案件ビュー・未分類ビュー）から
      // 除去し、移動したことが一目で分かるようにする。失敗分は残して再操作
      // できるようにする（削除フローと同じ removeMailFromState を再利用）
      set((state) => {
        let next: MailState = state;
        for (const mailId of result.succeeded) {
          next = { ...next, ...removeMailFromState(next, mailId) };
        }
        return next;
      });
      reportBulkResult(result, "案件への移動");
      return result;
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
      return null;
    }
  },
}));

/** 一括操作の結果をトーストで要約表示する。失敗が1件でも混在する場合は
 * 成功件数を含めてもエラートーストにする（一部失敗を成功扱いに見せない） */
function reportBulkResult(result: BulkResult, actionLabel: string): void {
  const { succeeded, failed } = result;
  if (failed.length === 0) {
    useErrorStore.getState().addSuccess(`${actionLabel}しました（${succeeded.length}件）`);
    return;
  }
  console.error(`bulk ${actionLabel} partial failure:`, failed);
  if (succeeded.length > 0) {
    useErrorStore
      .getState()
      .addError(`${actionLabel}しました（${succeeded.length}件、失敗 ${failed.length}件）`);
  } else {
    useErrorStore.getState().addError(`${actionLabel}に失敗しました（${failed.length}件）`);
  }
}
