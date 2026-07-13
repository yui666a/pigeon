import { invokeCommand } from "./client";
import type { Project } from "../types/project";

/** 案件（プロジェクト）CRUD 系 Tauri commands の型付きラッパ */
export const projectApi = {
  fetchProjects: (accountId: string) =>
    invokeCommand<Project[]>("get_projects", { accountId }),

  createProject: (
    accountId: string,
    name: string,
    description?: string,
    color?: string,
  ) =>
    invokeCommand<Project>("create_project", {
      accountId,
      name,
      description: description ?? null,
      color: color ?? null,
    }),

  updateProject: (id: string, name?: string, description?: string, color?: string) =>
    invokeCommand<void>("update_project", {
      id,
      name: name ?? null,
      description: description ?? null,
      color: color ?? null,
    }),

  archiveProject: (id: string) => invokeCommand<void>("archive_project", { id }),

  deleteProject: (id: string) => invokeCommand<void>("delete_project", { id }),

  /** source の全メールを target へ移動して source を削除する。移動件数を返す */
  mergeProjects: (sourceId: string, targetId: string) =>
    invokeCommand<number>("merge_projects", { sourceId, targetId }),
};
