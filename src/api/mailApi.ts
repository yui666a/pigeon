import { invokeCommand } from "./client";
import type {
  BackfillOutcome,
  BulkResult,
  SendMailRequest,
  ThreadPage,
  UnreadCounts,
} from "../types/mail";

/**
 * メール系 Tauri commands の型付きラッパ。
 * コマンド名・引数の組み立て・戻り型をここに固定する。
 */
export const mailApi = {
  /** スレッド一覧を1ページ分取得する（切り出しはスレッド単位・ADR 0006 決定5） */
  fetchThreads: (accountId: string, folder: string, limit: number, offset: number) =>
    invokeCommand<ThreadPage>("get_threads", { accountId, folder, limit, offset }),

  fetchThreadsByProject: (projectId: string, limit: number, offset: number) =>
    invokeCommand<ThreadPage>("get_threads_by_project", { projectId, limit, offset }),

  /** 取り込んだ件数を返す */
  syncAccount: (accountId: string) =>
    invokeCommand<number>("sync_account", { accountId }),

  /** ローカル最古より古いメールを limit 件まで遡って取得する */
  backfillAccount: (accountId: string, limit: number) =>
    invokeCommand<BackfillOutcome>("backfill_account", { accountId, limit }),

  markRead: (accountId: string, mailId: string) =>
    invokeCommand<void>("mark_read", { accountId, mailId }),

  markUnread: (accountId: string, mailId: string) =>
    invokeCommand<void>("mark_unread", { accountId, mailId }),

  setFlagged: (accountId: string, mailId: string, flagged: boolean) =>
    invokeCommand<void>("set_flagged", { accountId, mailId, flagged }),

  deleteMail: (accountId: string, mailId: string) =>
    invokeCommand<void>("delete_mail", { accountId, mailId }),

  archiveMail: (accountId: string, mailId: string) =>
    invokeCommand<void>("archive_mail", { accountId, mailId }),

  unarchiveMail: (accountId: string, mailId: string) =>
    invokeCommand<void>("unarchive_mail", { accountId, mailId }),

  fetchUnreadCounts: (accountId: string) =>
    invokeCommand<UnreadCounts>("get_unread_counts", { accountId }),

  fetchUnclassifiedThreads: (accountId: string, limit: number, offset: number) =>
    invokeCommand<ThreadPage>("get_unclassified_threads", { accountId, limit, offset }),

  bulkDeleteMails: (accountId: string, mailIds: string[]) =>
    invokeCommand<BulkResult>("bulk_delete_mails", { accountId, mailIds }),

  bulkArchiveMails: (accountId: string, mailIds: string[]) =>
    invokeCommand<BulkResult>("bulk_archive_mails", { accountId, mailIds }),

  bulkMoveMails: (mailIds: string[], projectId: string) =>
    invokeCommand<BulkResult>("bulk_move_mails", { mailIds, projectId }),

  sendMail: (req: SendMailRequest) =>
    invokeCommand<void>("send_mail", { req }),

  /** デスクトップ通知の件名プレビュー用。直近の未読件名を新しい順に返す */
  fetchRecentUnreadSubjects: (accountId: string, limit: number) =>
    invokeCommand<string[]>("get_recent_unread_subjects", { accountId, limit }),
};
