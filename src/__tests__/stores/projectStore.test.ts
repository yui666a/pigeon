import { describe, it, expect, vi, beforeEach } from "vitest";
import { useProjectStore } from "../../stores/projectStore";

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
    });
  });

  describe("fetchProjects", () => {
    it("sets projects on success", async () => {
      const projects = [
        { id: "p1", account_id: "acc1", name: "Project A", description: null, color: null, is_archived: false, created_at: "", updated_at: "" },
      ];
      mockInvoke.mockResolvedValue(projects);

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
});
