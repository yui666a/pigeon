import { create } from "zustand";
import type { Project } from "../types/project";
import type { ProjectContext, ProjectDirectory } from "../types/directory";
import { projectApi } from "../api/projectApi";
import { directoryApi } from "../api/directoryApi";
import { errorMessage } from "../api/errors";
import { useErrorStore } from "./errorStore";

const EXPANDED_PROJECTS_KEY = "pigeon.expandedProjects";

function loadExpandedIds(): Set<string> {
  try {
    const raw = localStorage.getItem(EXPANDED_PROJECTS_KEY);
    if (!raw) return new Set();
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? new Set(parsed.filter((v) => typeof v === "string")) : new Set();
  } catch {
    return new Set();
  }
}

function saveExpandedIds(ids: Set<string>): void {
  try {
    localStorage.setItem(EXPANDED_PROJECTS_KEY, JSON.stringify([...ids]));
  } catch {
    // localStorage 不可（プライベートモード等）は無視。永続化しないだけで機能は継続する
  }
}

/** 構造変更操作の成功後: 一覧再取得、消えた案件IDのキャッシュ掃除、選択解除
 *
 * projects は常に単一アカウントにスコープされる（fetchProjects(accountId) 経由でのみ populate される）ため、
 * 呼び出し側が accountId を持ち回さずとも既存の一覧から復元できる。
 */
async function refreshAfterStructuralChange(
  accountId: string,
  set: (partial: Partial<ProjectState>) => void,
  get: () => ProjectState,
): Promise<void> {
  await get().fetchProjects(accountId);
  const liveIds = new Set(get().projects.map((p) => p.id));

  const pruneRecord = <T,>(record: Record<string, T>): Record<string, T> => {
    const next: Record<string, T> = {};
    for (const [id, value] of Object.entries(record)) {
      if (liveIds.has(id)) next[id] = value;
    }
    return next;
  };

  set({
    directories: pruneRecord(get().directories),
    contexts: pruneRecord(get().contexts),
    scanningProjects: pruneRecord(get().scanningProjects),
    selectedProjectId:
      get().selectedProjectId && !liveIds.has(get().selectedProjectId as string)
        ? null
        : get().selectedProjectId,
  });
}

interface ProjectState {
  projects: Project[];
  selectedProjectId: string | null;
  loading: boolean;
  directories: Record<string, ProjectDirectory | null>;
  contexts: Record<string, ProjectContext | null>;
  scanningProjects: Record<string, boolean>;
  expandedIds: Set<string>;
  fetchProjects: (accountId: string) => Promise<void>;
  createProject: (
    accountId: string,
    name: string,
    description?: string,
    color?: string,
    parentId?: string | null,
  ) => Promise<Project>;
  updateProject: (
    id: string,
    name?: string,
    description?: string,
    color?: string,
  ) => Promise<void>;
  setProjectParent: (projectId: string, parentId: string | null) => Promise<void>;
  archiveProject: (id: string) => Promise<void>;
  deleteProject: (id: string) => Promise<void>;
  mergeProject: (sourceId: string, targetId: string) => Promise<number>;
  addProject: (project: Project) => void;
  selectProject: (id: string | null) => void;
  toggleExpanded: (id: string) => void;
  fetchDirectory: (projectId: string) => Promise<void>;
  linkDirectory: (projectId: string, path: string) => Promise<void>;
  unlinkDirectory: (projectId: string) => Promise<void>;
  rescanProject: (projectId: string) => Promise<void>;
  fetchProjectContext: (projectId: string) => Promise<void>;
  setAllowCloudContext: (projectId: string, allow: boolean) => Promise<void>;
}

export const useProjectStore = create<ProjectState>((set, get) => ({
  projects: [],
  selectedProjectId: null,
  loading: false,
  directories: {},
  contexts: {},
  scanningProjects: {},
  expandedIds: loadExpandedIds(),

  /** 案件一覧と各案件の主ディレクトリを 1 往復で取得する。
   *
   * 案件ごとに fetchDirectory を呼ぶと IPC が案件数ぶん往復し、かつ 1 件ごとの set で
   * サイドバーが案件数ぶん再レンダリングされる（ツリー構築と未読集約が毎回再計算）。
   * 集約コマンドで取得し、projects と directories を 1 回の set で反映する。 */
  fetchProjects: async (accountId) => {
    set({ loading: true });
    try {
      const rows = await projectApi.fetchProjectsWithDirectories(accountId);
      const projects: Project[] = [];
      const directories: Record<string, ProjectDirectory | null> = {};
      for (const { directory, ...project } of rows) {
        projects.push(project);
        directories[project.id] = directory;
      }
      set({ projects, directories, loading: false });
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  createProject: async (accountId, name, description, color, parentId) => {
    set({ loading: true });
    try {
      const project = await projectApi.createProject(
        accountId,
        name,
        description,
        color,
        parentId,
      );
      await get().fetchProjects(accountId);
      return project;
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(errorMessage(e));
      throw e;
    }
  },

  setProjectParent: async (projectId, parentId) => {
    const accountId = get().projects.find((p) => p.id === projectId)?.account_id;
    set({ loading: true });
    try {
      await projectApi.setProjectParent(projectId, parentId);
      if (accountId) await get().fetchProjects(accountId);
      else set({ loading: false });
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(errorMessage(e));
      throw e;
    }
  },

  updateProject: async (id, name, description, color) => {
    set({ loading: true });
    try {
      await projectApi.updateProject(id, name, description, color);
      const projects = get().projects.map((p) =>
        p.id === id
          ? {
              ...p,
              ...(name !== undefined && { name }),
              ...(description !== undefined && { description }),
              ...(color !== undefined && { color }),
            }
          : p,
      );
      set({ projects, loading: false });
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  archiveProject: async (id) => {
    const accountId = get().projects.find((p) => p.id === id)?.account_id;
    set({ loading: true });
    try {
      await projectApi.archiveProject(id);
      if (accountId) await refreshAfterStructuralChange(accountId, set, get);
      set({ loading: false });
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  deleteProject: async (id) => {
    const accountId = get().projects.find((p) => p.id === id)?.account_id;
    set({ loading: true });
    try {
      await projectApi.deleteProject(id);
      if (accountId) await refreshAfterStructuralChange(accountId, set, get);
      set({ loading: false });
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  mergeProject: async (sourceId, targetId) => {
    const accountId = get().projects.find((p) => p.id === sourceId)?.account_id;
    set({ loading: true });
    try {
      const moved = await projectApi.mergeProjects(sourceId, targetId);
      if (accountId) await refreshAfterStructuralChange(accountId, set, get);
      set({ loading: false });
      return moved;
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(errorMessage(e));
      throw e;
    }
  },

  addProject: (project) => {
    if (get().projects.some((p) => p.id === project.id)) return;
    set({ projects: [...get().projects, project] });
  },

  selectProject: (id) => set({ selectedProjectId: id }),

  toggleExpanded: (id) => {
    const next = new Set(get().expandedIds);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    saveExpandedIds(next);
    set({ expandedIds: next });
  },

  fetchDirectory: async (projectId) => {
    try {
      const dir = await directoryApi.fetchDirectory(projectId);
      if (!get().projects.some((p) => p.id === projectId)) return;
      set({ directories: { ...get().directories, [projectId]: dir } });
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  linkDirectory: async (projectId, path) => {
    try {
      const dir = await directoryApi.linkDirectory(projectId, path);
      set({ directories: { ...get().directories, [projectId]: dir } });
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
      throw e;
    }
  },

  unlinkDirectory: async (projectId) => {
    try {
      await directoryApi.unlinkDirectory(projectId);
      set({ directories: { ...get().directories, [projectId]: null } });
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  rescanProject: async (projectId) => {
    set({ scanningProjects: { ...get().scanningProjects, [projectId]: true } });
    try {
      await directoryApi.rescanDirectory(projectId);
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    } finally {
      const { [projectId]: _removed, ...rest } = get().scanningProjects;
      set({ scanningProjects: rest });
      void get().fetchDirectory(projectId);
      void get().fetchProjectContext(projectId);
    }
  },

  fetchProjectContext: async (projectId) => {
    try {
      const context = await directoryApi.fetchProjectContext(projectId);
      if (!get().projects.some((p) => p.id === projectId)) return;
      set({ contexts: { ...get().contexts, [projectId]: context } });
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  setAllowCloudContext: async (projectId, allow) => {
    try {
      await directoryApi.setAllowCloudContext(projectId, allow);
      await get().fetchProjectContext(projectId);
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  },
}));
