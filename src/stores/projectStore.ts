import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Project } from "../types/project";

interface ProjectState {
  projects: Project[];
  selectedProjectId: string | null;
  loading: boolean;
  error: string | null;
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
  selectProject: (id: string | null) => void;
}

export const useProjectStore = create<ProjectState>((set, get) => ({
  projects: [],
  selectedProjectId: null,
  loading: false,
  error: null,

  fetchProjects: async (accountId) => {
    set({ loading: true, error: null });
    try {
      const projects = await invoke<Project[]>("list_projects", { accountId });
      set({ projects, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  createProject: async (accountId, name, description, color) => {
    set({ loading: true, error: null });
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
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  updateProject: async (id, name, description, color) => {
    set({ loading: true, error: null });
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
      set({ error: String(e), loading: false });
    }
  },

  archiveProject: async (id) => {
    set({ loading: true, error: null });
    try {
      await invoke("archive_project", { id });
      set({
        projects: get().projects.filter((p) => p.id !== id),
        selectedProjectId:
          get().selectedProjectId === id ? null : get().selectedProjectId,
        loading: false,
      });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  deleteProject: async (id) => {
    set({ loading: true, error: null });
    try {
      await invoke("delete_project", { id });
      set({
        projects: get().projects.filter((p) => p.id !== id),
        selectedProjectId:
          get().selectedProjectId === id ? null : get().selectedProjectId,
        loading: false,
      });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  selectProject: (id) => set({ selectedProjectId: id }),
}));
