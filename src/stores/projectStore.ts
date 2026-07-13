import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Project } from "../types/project";
import type { ProjectContext, ProjectDirectory, RescanOutcome } from "../types/directory";
import { useErrorStore } from "./errorStore";

interface ProjectState {
  projects: Project[];
  selectedProjectId: string | null;
  loading: boolean;
  directories: Record<string, ProjectDirectory | null>;
  contexts: Record<string, ProjectContext | null>;
  scanningProjects: Record<string, boolean>;
  fetchProjects: (accountId: string) => Promise<void>;
  createProject: (
    accountId: string,
    name: string,
    description?: string,
    color?: string,
  ) => Promise<Project>;
  updateProject: (
    id: string,
    name?: string,
    description?: string,
    color?: string,
  ) => Promise<void>;
  archiveProject: (id: string) => Promise<void>;
  deleteProject: (id: string) => Promise<void>;
  mergeProject: (sourceId: string, targetId: string) => Promise<number>;
  addProject: (project: Project) => void;
  selectProject: (id: string | null) => void;
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

  fetchProjects: async (accountId) => {
    set({ loading: true });
    try {
      const projects = await invoke<Project[]>("get_projects", { accountId });
      set({ projects, loading: false });
      for (const p of projects) {
        void get().fetchDirectory(p.id);
      }
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(String(e));
    }
  },

  createProject: async (accountId, name, description, color) => {
    set({ loading: true });
    try {
      const project = await invoke<Project>("create_project", {
        accountId,
        name,
        description: description ?? null,
        color: color ?? null,
      });
      await get().fetchProjects(accountId);
      return project;
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(String(e));
      throw e;
    }
  },

  updateProject: async (id, name, description, color) => {
    set({ loading: true });
    try {
      await invoke("update_project", {
        id,
        name: name ?? null,
        description: description ?? null,
        color: color ?? null,
      });
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
      useErrorStore.getState().addError(String(e));
    }
  },

  archiveProject: async (id) => {
    set({ loading: true });
    try {
      await invoke("archive_project", { id });
      set({
        projects: get().projects.filter((p) => p.id !== id),
        selectedProjectId:
          get().selectedProjectId === id ? null : get().selectedProjectId,
        loading: false,
      });
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(String(e));
    }
  },

  deleteProject: async (id) => {
    set({ loading: true });
    try {
      await invoke("delete_project", { id });
      set({
        projects: get().projects.filter((p) => p.id !== id),
        selectedProjectId:
          get().selectedProjectId === id ? null : get().selectedProjectId,
        loading: false,
      });
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(String(e));
    }
  },

  mergeProject: async (sourceId, targetId) => {
    set({ loading: true });
    try {
      const moved = await invoke<number>("merge_projects", {
        sourceId,
        targetId,
      });
      set({
        projects: get().projects.filter((p) => p.id !== sourceId),
        selectedProjectId:
          get().selectedProjectId === sourceId ? targetId : get().selectedProjectId,
        loading: false,
      });
      return moved;
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(String(e));
      throw e;
    }
  },

  addProject: (project) => {
    if (get().projects.some((p) => p.id === project.id)) return;
    set({ projects: [...get().projects, project] });
  },

  selectProject: (id) => set({ selectedProjectId: id }),

  fetchDirectory: async (projectId) => {
    try {
      const dir = await invoke<ProjectDirectory | null>("get_project_directory", {
        projectId,
      });
      set({ directories: { ...get().directories, [projectId]: dir } });
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  },

  linkDirectory: async (projectId, path) => {
    try {
      const dir = await invoke<ProjectDirectory>("link_project_directory", {
        projectId,
        path,
      });
      set({ directories: { ...get().directories, [projectId]: dir } });
    } catch (e) {
      useErrorStore.getState().addError(String(e));
      throw e;
    }
  },

  unlinkDirectory: async (projectId) => {
    try {
      await invoke("unlink_project_directory", { projectId });
      set({ directories: { ...get().directories, [projectId]: null } });
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  },

  rescanProject: async (projectId) => {
    set({ scanningProjects: { ...get().scanningProjects, [projectId]: true } });
    try {
      await invoke<RescanOutcome>("rescan_project_directory", { projectId });
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    } finally {
      const { [projectId]: _removed, ...rest } = get().scanningProjects;
      set({ scanningProjects: rest });
      void get().fetchDirectory(projectId);
      void get().fetchProjectContext(projectId);
    }
  },

  fetchProjectContext: async (projectId) => {
    try {
      const context = await invoke<ProjectContext | null>("get_project_context", {
        projectId,
      });
      set({ contexts: { ...get().contexts, [projectId]: context } });
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  },

  setAllowCloudContext: async (projectId, allow) => {
    try {
      await invoke("set_allow_cloud_context", { projectId, allow });
      await get().fetchProjectContext(projectId);
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  },
}));
