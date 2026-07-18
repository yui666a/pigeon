import { invokeCommand } from "./client";
import type { DeleteImpact, EffectiveContextEntry, Project } from "../types/project";

/** 案件（プロジェクト）CRUD 系 Tauri commands の型付きラッパ */
export const projectApi = {
  fetchProjects: (accountId: string) =>
    invokeCommand<Project[]>("get_projects", { accountId }),

  createProject: (
    accountId: string,
    name: string,
    description?: string,
    color?: string,
    parentId?: string | null,
  ) =>
    invokeCommand<Project>("create_project", {
      accountId,
      name,
      description: description ?? null,
      color: color ?? null,
      parentId: parentId ?? null,
    }),

  updateProject: (id: string, name?: string, description?: string, color?: string) =>
    invokeCommand<void>("update_project", {
      id,
      name: name ?? null,
      description: description ?? null,
      color: color ?? null,
    }),

  /** 親案件を変更する。parentId が null ならルートに移動する */
  setProjectParent: (projectId: string, parentId: string | null) =>
    invokeCommand<void>("set_project_parent", { projectId, parentId }),

  archiveProject: (id: string) => invokeCommand<void>("archive_project", { id }),

  deleteProject: (id: string) => invokeCommand<void>("delete_project", { id }),

  /** source の全メールを target へ移動して source を削除する。移動件数を返す */
  mergeProjects: (sourceId: string, targetId: string) =>
    invokeCommand<number>("merge_projects", { sourceId, targetId }),

  /** 削除確認ダイアログ用: サブツリーの案件数とメール件数 */
  getProjectDeleteImpact: (projectId: string) =>
    invokeCommand<DeleteImpact>("get_project_delete_impact", { projectId }),

  /** 祖先パスに沿った加算的な有効コンテキスト */
  getEffectiveContext: (projectId: string) =>
    invokeCommand<EffectiveContextEntry[]>("get_effective_context", { projectId }),
};
