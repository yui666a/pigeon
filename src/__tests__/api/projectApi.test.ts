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
    });
    expect(created).toEqual(project);
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
});
