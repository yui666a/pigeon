import { describe, it, expect, vi, beforeEach } from "vitest";
import { useProjectStore } from "../../stores/projectStore";
import { useErrorStore } from "../../stores/errorStore";
import type { ProjectDirectory } from "../../types/directory";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

describe("projectStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    useProjectStore.setState({
      projects: [],
      selectedProjectId: null,
      loading: false,
      directories: {},
      contexts: {},
      scanningProjects: {},
      expandedIds: new Set(),
    });
    useErrorStore.setState({ toasts: [] });
  });

  describe("fetchProjects", () => {
    it("sets projects on success", async () => {
      const projects = [
        { id: "p1", account_id: "acc1", name: "Project A", description: null, color: null, is_archived: false, parent_id: null, created_at: "", updated_at: "" },
      ];
      mockInvoke.mockImplementation((cmd: unknown) => {
        if (cmd === "get_projects") return Promise.resolve(projects);
        return Promise.resolve(null);
      });

      await useProjectStore.getState().fetchProjects("acc1");

      expect(mockInvoke).toHaveBeenCalledWith("get_projects", { accountId: "acc1" });
      expect(useProjectStore.getState().projects).toEqual(projects);
      expect(useProjectStore.getState().loading).toBe(false);
    });

    it("reports an error toast on failure", async () => {
      mockInvoke.mockRejectedValue("DB error");

      await useProjectStore.getState().fetchProjects("acc1");

      expect(useProjectStore.getState().loading).toBe(false);
      const toasts = useErrorStore.getState().toasts;
      expect(toasts).toHaveLength(1);
      expect(toasts[0]).toMatchObject({ kind: "error", message: "DB error" });
    });
  });

  describe("selectProject", () => {
    it("sets selectedProjectId", () => {
      useProjectStore.getState().selectProject("p1");
      expect(useProjectStore.getState().selectedProjectId).toBe("p1");
    });

    it("clears selectedProjectId with null", () => {
      useProjectStore.getState().selectProject("p1");
      useProjectStore.getState().selectProject(null);
      expect(useProjectStore.getState().selectedProjectId).toBeNull();
    });
  });

  describe("deleteProject", () => {
    const p1 = { id: "p1", account_id: "acc1", name: "A", description: null, color: null, is_archived: false, parent_id: null, created_at: "", updated_at: "" };
    const p2 = { id: "p2", account_id: "acc1", name: "B", description: null, color: null, is_archived: false, parent_id: null, created_at: "", updated_at: "" };

    it("removes project from list and clears selection if selected", async () => {
      useProjectStore.setState({
        projects: [p1, p2],
        selectedProjectId: "p1",
      });
      mockInvoke.mockImplementation((cmd: unknown) => {
        if (cmd === "delete_project") return Promise.resolve(undefined);
        if (cmd === "get_projects") return Promise.resolve([p2]);
        return Promise.resolve(null);
      });

      await useProjectStore.getState().deleteProject("p1");

      expect(useProjectStore.getState().projects).toHaveLength(1);
      expect(useProjectStore.getState().projects[0].id).toBe("p2");
      expect(useProjectStore.getState().selectedProjectId).toBeNull();
    });

    it("keeps selection when deleting a different project", async () => {
      useProjectStore.setState({
        projects: [p1, p2],
        selectedProjectId: "p1",
      });
      mockInvoke.mockImplementation((cmd: unknown) => {
        if (cmd === "delete_project") return Promise.resolve(undefined);
        if (cmd === "get_projects") return Promise.resolve([p1]);
        return Promise.resolve(null);
      });

      await useProjectStore.getState().deleteProject("p2");

      expect(useProjectStore.getState().selectedProjectId).toBe("p1");
    });

    it("prunes cache entries for ids no longer present after refetch", async () => {
      useProjectStore.setState({
        projects: [p1, p2],
        directories: { p1: null, p2: null },
        contexts: { p1: null, p2: null },
        scanningProjects: { p1: true, p2: false },
      });
      mockInvoke.mockImplementation((cmd: unknown) => {
        if (cmd === "delete_project") return Promise.resolve(undefined);
        if (cmd === "get_projects") return Promise.resolve([p2]);
        return Promise.resolve(null);
      });

      await useProjectStore.getState().deleteProject("p1");

      expect(useProjectStore.getState().directories).toEqual({ p2: null });
      expect(useProjectStore.getState().contexts).toEqual({ p2: null });
      expect(useProjectStore.getState().scanningProjects).toEqual({ p2: false });
    });
  });

  describe("archiveProject", () => {
    it("removes project from list", async () => {
      const p1 = { id: "p1", account_id: "acc1", name: "A", description: null, color: null, is_archived: false, parent_id: null, created_at: "", updated_at: "" };
      useProjectStore.setState({
        projects: [p1],
        selectedProjectId: "p1",
      });
      mockInvoke.mockImplementation((cmd: unknown) => {
        if (cmd === "archive_project") return Promise.resolve(undefined);
        if (cmd === "get_projects") return Promise.resolve([]);
        return Promise.resolve(null);
      });

      await useProjectStore.getState().archiveProject("p1");

      expect(useProjectStore.getState().projects).toHaveLength(0);
      expect(useProjectStore.getState().selectedProjectId).toBeNull();
    });
  });

  describe("setProjectParent", () => {
    it("invokes set_project_parent then refetches projects", async () => {
      const p1 = { id: "p1", account_id: "acc1", name: "A", description: null, color: null, is_archived: false, parent_id: null, created_at: "", updated_at: "" };
      const moved = { ...p1, parent_id: "root" };
      useProjectStore.setState({ projects: [p1] });
      mockInvoke.mockImplementation((cmd: unknown) => {
        if (cmd === "set_project_parent") return Promise.resolve(undefined);
        if (cmd === "get_projects") return Promise.resolve([moved]);
        return Promise.resolve(null);
      });

      await useProjectStore.getState().setProjectParent("p1", "root");

      expect(mockInvoke).toHaveBeenCalledWith("set_project_parent", {
        projectId: "p1",
        parentId: "root",
      });
      expect(useProjectStore.getState().projects[0].parent_id).toBe("root");
    });
  });

  describe("toggleExpanded", () => {
    it("adds an id when not expanded, removes it when expanded", () => {
      useProjectStore.getState().toggleExpanded("p1");
      expect(useProjectStore.getState().expandedIds.has("p1")).toBe(true);

      useProjectStore.getState().toggleExpanded("p1");
      expect(useProjectStore.getState().expandedIds.has("p1")).toBe(false);
    });

    it("persists to localStorage", () => {
      useProjectStore.getState().toggleExpanded("p1");
      const stored = JSON.parse(localStorage.getItem("pigeon.expandedProjects") ?? "[]");
      expect(stored).toEqual(["p1"]);
    });
  });

  describe("directory linkage", () => {
    const dir: ProjectDirectory = {
      id: "d1",
      project_id: "p1",
      path: "/tmp/stage-a",
      is_primary: true,
      status: "ok",
      last_scanned_at: null,
      created_at: "",
    };
    const p1 = { id: "p1", account_id: "acc1", name: "A", description: null, color: null, is_archived: false, parent_id: null, created_at: "", updated_at: "" };

    beforeEach(() => {
      useProjectStore.setState({ projects: [p1] });
    });

    it("fetchDirectory stores the linked directory", async () => {
      mockInvoke.mockResolvedValue(dir);
      await useProjectStore.getState().fetchDirectory("p1");
      expect(mockInvoke).toHaveBeenCalledWith("get_project_directory", { projectId: "p1" });
      expect(useProjectStore.getState().directories["p1"]).toEqual(dir);
    });

    it("fetchDirectory stores null when unlinked", async () => {
      mockInvoke.mockResolvedValue(null);
      await useProjectStore.getState().fetchDirectory("p1");
      expect(useProjectStore.getState().directories["p1"]).toBeNull();
    });

    it("fetchDirectory discards the response if the project is no longer in the list", async () => {
      useProjectStore.setState({ projects: [] });
      mockInvoke.mockResolvedValue(dir);
      await useProjectStore.getState().fetchDirectory("p1");
      expect(useProjectStore.getState().directories["p1"]).toBeUndefined();
    });

    it("linkDirectory invokes command and refreshes directory", async () => {
      mockInvoke.mockResolvedValue(dir);
      await useProjectStore.getState().linkDirectory("p1", "/tmp/stage-a");
      expect(mockInvoke).toHaveBeenCalledWith("link_project_directory", {
        projectId: "p1",
        path: "/tmp/stage-a",
      });
      expect(useProjectStore.getState().directories["p1"]).toEqual(dir);
    });

    it("unlinkDirectory clears the entry", async () => {
      useProjectStore.setState({ directories: { p1: dir } });
      mockInvoke.mockResolvedValue(undefined);
      await useProjectStore.getState().unlinkDirectory("p1");
      expect(mockInvoke).toHaveBeenCalledWith("unlink_project_directory", { projectId: "p1" });
      expect(useProjectStore.getState().directories["p1"]).toBeNull();
    });

    it("rescanProject toggles scanning flag and refreshes state", async () => {
      let resolveRescan: (v: unknown) => void = () => {};
      mockInvoke.mockImplementation((cmd: unknown) => {
        if (cmd === "rescan_project_directory") {
          return new Promise((resolve) => { resolveRescan = resolve; });
        }
        return Promise.resolve(null);
      });

      const promise = useProjectStore.getState().rescanProject("p1");
      expect(useProjectStore.getState().scanningProjects["p1"]).toBe(true);

      resolveRescan({ status: "ok", regenerated: true, file_count: 3 });
      await promise;
      expect(useProjectStore.getState().scanningProjects["p1"]).toBeUndefined();
      expect(mockInvoke).toHaveBeenCalledWith("rescan_project_directory", { projectId: "p1" });
    });

    it("setAllowCloudContext invokes command and refreshes context", async () => {
      mockInvoke.mockResolvedValue(null);
      await useProjectStore.getState().setAllowCloudContext("p1", true);
      expect(mockInvoke).toHaveBeenCalledWith("set_allow_cloud_context", {
        projectId: "p1",
        allow: true,
      });
    });
  });

  describe("addProject", () => {
    beforeEach(() => {
      useProjectStore.setState({ projects: [] });
    });

    it("既存配列にプロジェクトを追加する", () => {
      const project = {
        id: "p1",
        account_id: "acc1",
        name: "Alpha",
        description: null,
        color: null,
        is_archived: false,
        parent_id: null,
        created_at: "",
        updated_at: "",
      };
      useProjectStore.getState().addProject(project);
      expect(useProjectStore.getState().projects).toHaveLength(1);
      expect(useProjectStore.getState().projects[0].id).toBe("p1");
    });

    it("同じIDは重複追加しない", () => {
      const project1 = {
        id: "p1",
        account_id: "acc1",
        name: "Alpha",
        description: null,
        color: null,
        is_archived: false,
        parent_id: null,
        created_at: "",
        updated_at: "",
      };
      const project2 = {
        id: "p1",
        account_id: "acc1",
        name: "Alpha dup",
        description: null,
        color: null,
        is_archived: false,
        parent_id: null,
        created_at: "",
        updated_at: "",
      };
      useProjectStore.getState().addProject(project1);
      useProjectStore.getState().addProject(project2);
      expect(useProjectStore.getState().projects).toHaveLength(1);
      expect(useProjectStore.getState().projects[0].name).toBe("Alpha");
    });
  });
});
