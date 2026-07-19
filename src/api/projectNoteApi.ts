import { invokeCommand } from "./client";
import type {
  AiHistoryEntry,
  GenerateNoteOutcome,
  ProjectNote,
} from "../types/projectNote";

/** 案件ノート（ユーザー手書き + AI要約）系の型付きラッパ */
export const projectNoteApi = {
  fetchNote: (projectId: string) =>
    invokeCommand<ProjectNote | null>("get_project_note", { projectId }),

  saveUserNote: (projectId: string, userMd: string) =>
    invokeCommand<void>("save_project_note_user", { projectId, userMd }),

  saveAiNote: (projectId: string, aiMd: string) =>
    invokeCommand<void>("save_project_note_ai", { projectId, aiMd }),

  /** cloud/local の判定はサーバー側の設定に基づく（クライアントからは渡さない） */
  generateAiNote: (projectId: string) =>
    invokeCommand<GenerateNoteOutcome>("generate_project_note_ai", { projectId }),

  fetchAiHistory: (projectId: string) =>
    invokeCommand<AiHistoryEntry[]>("list_project_note_ai_history", { projectId }),

  restoreAiNote: (historyId: string) =>
    invokeCommand<void>("restore_project_note_ai", { historyId }),
};
