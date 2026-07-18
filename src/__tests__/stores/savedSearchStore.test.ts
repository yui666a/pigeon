import { describe, it, expect, vi, beforeEach } from "vitest";
import { useSavedSearchStore } from "../../stores/savedSearchStore";
import { useErrorStore } from "../../stores/errorStore";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

const sample = {
  id: 1,
  name: "照明",
  query: "灯体",
  mode: "semantic",
  sort_order: 0,
  created_at: "2026-07-18",
};

describe("savedSearchStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSavedSearchStore.setState({ savedSearches: [], loading: false });
    useErrorStore.setState({ toasts: [] });
  });

  it("fetch で一覧を取得する", async () => {
    mockInvoke.mockResolvedValue([sample]);
    await useSavedSearchStore.getState().fetch();
    expect(mockInvoke).toHaveBeenCalledWith("list_saved_searches", {});
    expect(useSavedSearchStore.getState().savedSearches).toEqual([sample]);
  });

  it("create は作成後に再取得する", async () => {
    mockInvoke.mockImplementation((cmd: string) =>
      cmd === "list_saved_searches"
        ? Promise.resolve([sample])
        : Promise.resolve(sample),
    );
    await useSavedSearchStore.getState().create("照明", "灯体", "semantic");
    expect(mockInvoke).toHaveBeenCalledWith("create_saved_search", {
      name: "照明",
      query: "灯体",
      mode: "semantic",
    });
    expect(useSavedSearchStore.getState().savedSearches).toEqual([sample]);
  });

  it("remove は削除後に再取得する", async () => {
    mockInvoke.mockImplementation((cmd: string) =>
      cmd === "list_saved_searches" ? Promise.resolve([]) : Promise.resolve(null),
    );
    await useSavedSearchStore.getState().remove(1);
    expect(mockInvoke).toHaveBeenCalledWith("delete_saved_search", { id: 1 });
  });

  it("rename は改名後に再取得する", async () => {
    mockInvoke.mockImplementation((cmd: string) =>
      cmd === "list_saved_searches"
        ? Promise.resolve([{ ...sample, name: "新名" }])
        : Promise.resolve(null),
    );
    await useSavedSearchStore.getState().rename(1, "新名");
    expect(mockInvoke).toHaveBeenCalledWith("rename_saved_search", {
      id: 1,
      name: "新名",
    });
    expect(useSavedSearchStore.getState().savedSearches[0].name).toBe("新名");
  });

  it("fetch 失敗時はエラーストアへ通知する", async () => {
    mockInvoke.mockRejectedValue("boom");
    await useSavedSearchStore.getState().fetch();
    expect(useErrorStore.getState().toasts.length).toBe(1);
    expect(useSavedSearchStore.getState().loading).toBe(false);
  });
});
