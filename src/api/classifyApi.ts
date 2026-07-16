import { invokeCommand } from "./client";
import type {
  ClassifyBatchOutcome,
  ClassifyResponse,
  ProjectSuggestion,
} from "../types/classifier";
import type { Project } from "../types/project";

/** AI 分類系 Tauri commands の型付きラッパ */
export const classifyApi = {
  /**
   * 1件を分類する。Rust の ClassifyResponse は mail_id と ClassifyResult が
   * 両方とも #[serde(flatten)] されているため、実際の JSON は
   * { mail_id, action, confidence, reason, ... } の完全にフラットな形になる
   * （result という入れ子は存在しない）。
   */
  classifyMail: (mailId: string) =>
    invokeCommand<ClassifyResponse>("classify_mail", { mailId }),

  /**
   * 未分類メールのバッチ分類を開始/再開する。
   * 1 invoke で「次の停止点（create 提案）or 完了/中断」まで進む。
   * 進捗は classify-progress イベントで届く。
   */
  classifyBatch: (accountId: string) =>
    invokeCommand<ClassifyBatchOutcome>("classify_batch", { accountId }),

  /** 実行中/承認待ちのバッチ分類を中止する */
  cancelClassification: (accountId: string) =>
    invokeCommand<void>("cancel_classification", { accountId }),

  /** 新規案件の提案を承認し、作成された案件を返す */
  approveNewProject: (mailId: string, projectName: string, description?: string) =>
    invokeCommand<Project>("approve_new_project", {
      mailId,
      projectName,
      description: description ?? null,
    }),

  rejectClassification: (mailId: string) =>
    invokeCommand<void>("reject_classification", { mailId }),

  /** 選択メール群から新規案件名・説明を LLM に提案させる */
  suggestProjectFromMails: (mailIds: string[]) =>
    invokeCommand<ProjectSuggestion>("suggest_project_from_mails", { mailIds }),
};
