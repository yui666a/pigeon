import { describe, it, expect, vi, beforeEach } from "vitest";
import { projectApi } from "../../api/projectApi";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

describe("projectApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("createProject は省略された description/color を null に変換して渡す", async () => {
    const project = { id: "p1", name: "案件A" };
    mockInvoke.mockResolvedValue(project);

    const created = await projectApi.createProject("acc1", "案件A");

    expect(mockInvoke).toHaveBeenCalledWith("create_project", {
      accountId: "acc1",
      name: "案件A",
      description: null,
      color: null,
      parentId: null,
    });
    expect(created).toEqual(project);
  });

  it("createProject は parentId を渡す", async () => {
    mockInvoke.mockResolvedValue({ id: "p2", name: "子案件" });

    await projectApi.createProject("acc1", "子案件", undefined, undefined, "p1");

    expect(mockInvoke).toHaveBeenCalledWith("create_project", {
      accountId: "acc1",
      name: "子案件",
      description: null,
      color: null,
      parentId: "p1",
    });
  });

  it("updateProject は省略項目を null に変換して渡す", async () => {
    mockInvoke.mockResolvedValue(undefined);

    await projectApi.updateProject("p1", "新名称");

    expect(mockInvoke).toHaveBeenCalledWith("update_project", {
      id: "p1",
      name: "新名称",
      description: null,
      color: null,
    });
  });

  it("mergeProjects は merge_projects を呼び、移動件数を返す", async () => {
    mockInvoke.mockResolvedValue(3);

    const moved = await projectApi.mergeProjects("src", "dst");

    expect(mockInvoke).toHaveBeenCalledWith("merge_projects", {
      sourceId: "src",
      targetId: "dst",
    });
    expect(moved).toBe(3);
  });

  it("setProjectParent は set_project_parent を呼ぶ", async () => {
    mockInvoke.mockResolvedValue(undefined);

    await projectApi.setProjectParent("p1", "p2");

    expect(mockInvoke).toHaveBeenCalledWith("set_project_parent", {
      projectId: "p1",
      parentId: "p2",
    });
  });

  it("setProjectParent は null でルートへ移動できる", async () => {
    mockInvoke.mockResolvedValue(undefined);

    await projectApi.setProjectParent("p1", null);

    expect(mockInvoke).toHaveBeenCalledWith("set_project_parent", {
      projectId: "p1",
      parentId: null,
    });
  });

  it("getProjectDeleteImpact は get_project_delete_impact を呼び、結果を返す", async () => {
    const impact = { projects: 3, mails: 42 };
    mockInvoke.mockResolvedValue(impact);

    const result = await projectApi.getProjectDeleteImpact("p1");

    expect(mockInvoke).toHaveBeenCalledWith("get_project_delete_impact", {
      projectId: "p1",
    });
    expect(result).toEqual(impact);
  });

  it("getEffectiveContext は get_effective_context を呼び、結果を返す", async () => {
    const entries = [
      { project_id: "p1", project_name: "A", is_self: true, directory_path: null, context: null },
    ];
    mockInvoke.mockResolvedValue(entries);

    const result = await projectApi.getEffectiveContext("p1");

    expect(mockInvoke).toHaveBeenCalledWith("get_effective_context", {
      projectId: "p1",
    });
    expect(result).toEqual(entries);
  });
});
