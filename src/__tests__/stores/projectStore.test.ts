import { describe, it, expect, vi, beforeEach } from "vitest";
import { useProjectStore } from "../../stores/projectStore";
import type { ProjectDirectory } from "../../types/directory";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

describe("projectStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useProjectStore.setState({
      projects: [],
      selectedProjectId: null,
      loading: false,
      error: null,
      directories: {},
      contexts: {},
      scanningProjects: {},
    });
  });

  describe("fetchProjects", () => {
    it("sets projects on success", async () => {
      const projects = [
        { id: "p1", account_id: "acc1", name: "Project A", description: null, color: null, is_archived: false, created_at: "", updated_at: "" },
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

    it("sets error on failure", async () => {
      mockInvoke.mockRejectedValue("DB error");

      await useProjectStore.getState().fetchProjects("acc1");

      expect(useProjectStore.getState().error).toBe("DB error");
      expect(useProjectStore.getState().loading).toBe(false);
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
    it("removes project from list and clears selection if selected", async () => {
      useProjectStore.setState({
        projects: [
          { id: "p1", account_id: "acc1", name: "A", description: null, color: null, is_archived: false, created_at: "", updated_at: "" },
          { id: "p2", account_id: "acc1", name: "B", description: null, color: null, is_archived: false, created_at: "", updated_at: "" },
        ],
        selectedProjectId: "p1",
      });
      mockInvoke.mockResolvedValue(undefined);

      await useProjectStore.getState().deleteProject("p1");

      expect(useProjectStore.getState().projects).toHaveLength(1);
      expect(useProjectStore.getState().projects[0].id).toBe("p2");
      expect(useProjectStore.getState().selectedProjectId).toBeNull();
    });

    it("keeps selection when deleting a different project", async () => {
      useProjectStore.setState({
        projects: [
          { id: "p1", account_id: "acc1", name: "A", description: null, color: null, is_archived: false, created_at: "", updated_at: "" },
          { id: "p2", account_id: "acc1", name: "B", description: null, color: null, is_archived: false, created_at: "", updated_at: "" },
        ],
        selectedProjectId: "p1",
      });
      mockInvoke.mockResolvedValue(undefined);

      await useProjectStore.getState().deleteProject("p2");

      expect(useProjectStore.getState().selectedProjectId).toBe("p1");
    });
  });

  describe("archiveProject", () => {
    it("removes project from list", async () => {
      useProjectStore.setState({
        projects: [
          { id: "p1", account_id: "acc1", name: "A", description: null, color: null, is_archived: false, created_at: "", updated_at: "" },
        ],
        selectedProjectId: "p1",
      });
      mockInvoke.mockResolvedValue(undefined);

      await useProjectStore.getState().archiveProject("p1");

      expect(useProjectStore.getState().projects).toHaveLength(0);
      expect(useProjectStore.getState().selectedProjectId).toBeNull();
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
