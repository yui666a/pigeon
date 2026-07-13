import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { useErrorStore } from "../stores/errorStore";

describe("errorStore", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useErrorStore.setState({ toasts: [] });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("addError でトーストが追加され、5秒後に自動で消える", () => {
    useErrorStore.getState().addError("エラーです");
    expect(useErrorStore.getState().toasts).toHaveLength(1);
    expect(useErrorStore.getState().toasts[0].kind).toBe("error");

    vi.advanceTimersByTime(5000);
    expect(useErrorStore.getState().toasts).toHaveLength(0);
  });

  it("addSuccess で成功トーストが追加される", () => {
    useErrorStore.getState().addSuccess("保存しました");
    expect(useErrorStore.getState().toasts[0].kind).toBe("success");
  });

  it("dismissToast で手動消去できる", () => {
    useErrorStore.getState().addError("エラーです");
    const id = useErrorStore.getState().toasts[0].id;
    useErrorStore.getState().dismissToast(id);
    expect(useErrorStore.getState().toasts).toHaveLength(0);
  });

  it("手動消去後は自動消滅タイマーが残らない（タイマーリークしない）", () => {
    useErrorStore.getState().addError("エラーです");
    const id = useErrorStore.getState().toasts[0].id;
    expect(vi.getTimerCount()).toBe(1);

    useErrorStore.getState().dismissToast(id);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("自動消滅後もタイマーが残らない", () => {
    useErrorStore.getState().addError("エラーです");
    vi.advanceTimersByTime(5000);
    expect(vi.getTimerCount()).toBe(0);
  });
});
