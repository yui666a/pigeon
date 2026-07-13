import { invokeCommand } from "./client";
import type {
  CloudRule,
  ProjectContext,
  ProjectDirectory,
  ProjectFile,
  RescanOutcome,
} from "../types/directory";

/** 案件ディレクトリ連携（スキャン・コンテキスト・クラウド送信可否）系の型付きラッパ */
export const directoryApi = {
  fetchDirectory: (projectId: string) =>
    invokeCommand<ProjectDirectory | null>("get_project_directory", { projectId }),

  linkDirectory: (projectId: string, path: string) =>
    invokeCommand<ProjectDirectory>("link_project_directory", { projectId, path }),

  unlinkDirectory: (projectId: string) =>
    invokeCommand<void>("unlink_project_directory", { projectId }),

  rescanDirectory: (projectId: string) =>
    invokeCommand<RescanOutcome>("rescan_project_directory", { projectId }),

  fetchProjectContext: (projectId: string) =>
    invokeCommand<ProjectContext | null>("get_project_context", { projectId }),

  setAllowCloudContext: (projectId: string, allow: boolean) =>
    invokeCommand<void>("set_allow_cloud_context", { projectId, allow }),

  listProjectFiles: (directoryId: string) =>
    invokeCommand<ProjectFile[]>("list_project_files", { directoryId }),

  fetchCloudRules: (directoryId: string) =>
    invokeCommand<CloudRule[]>("get_cloud_rules", { directoryId }),

  /** allow が null のときはルール削除（継承に戻す） */
  setCloudRule: (
    directoryId: string,
    scope: "directory" | "file",
    relativePath: string,
    allow: boolean | null,
  ) =>
    invokeCommand<void>("set_cloud_rule", {
      directoryId,
      scope,
      relativePath,
      allow,
    }),
};
