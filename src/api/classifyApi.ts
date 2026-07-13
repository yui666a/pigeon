import { invokeCommand } from "./client";
import type { ClassifyResponse, UnclassifiedMailRef } from "../types/classifier";
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

  fetchUnclassifiedMailRefs: (accountId: string) =>
    invokeCommand<UnclassifiedMailRef[]>("get_unclassified_mails", { accountId }),

  /** 新規案件の提案を承認し、作成された案件を返す */
  approveNewProject: (mailId: string, projectName: string, description?: string) =>
    invokeCommand<Project>("approve_new_project", {
      mailId,
      projectName,
      description: description ?? null,
    }),

  rejectClassification: (mailId: string) =>
    invokeCommand<void>("reject_classification", { mailId }),
};
