import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { useCreateProjectFromSelection } from "../hooks/useCreateProjectFromSelection";
import { useProjectStore } from "../stores/projectStore";
import { useMailStore } from "../stores/mailStore";
import { useSelectionStore } from "../stores/selectionStore";
import type { Thread } from "../types/mail";
import type { Project } from "../types/project";

// フックが読むのは thread_id と mails[].id だけなので、その2つだけ持つ最小構造で足りる。
function makeThread(threadId: string, mailIds: string[]): Thread {
  return {
    thread_id: threadId,
    mails: mailIds.map((id) => ({ id })),
  } as unknown as Thread;
}

const makeProject = (id: string): Project => ({
  id,
  account_id: "acc1",
  name: "新案件",
  description: null,
  color: null,
  is_archived: false,
  created_at: "2026-07-17T00:00:00",
  updated_at: "2026-07-17T00:00:00",
});

describe("useCreateProjectFromSelection", () => {
  beforeEach(() => {
    useSelectionStore.setState({ selectedThreadIds: new Set() });
    vi.restoreAllMocks();
  });

  it("最初は creating=false・formMailIds 空", () => {
    const { result } = renderHook(() =>
      useCreateProjectFromSelection({
        accountId: "acc1",
        threads: [],
        reload: () => {},
      }),
    );
    expect(result.current.creating).toBe(false);
    expect(result.current.formMailIds).toEqual([]);
  });

  it("open() は選択スレッドの mailId を固定して creating=true にする", () => {
    useSelectionStore.setState({ selectedThreadIds: new Set(["t1"]) });
    const threads = [makeThread("t1", ["m1", "m2"])];
    const { result } = renderHook(() =>
      useCreateProjectFromSelection({
        accountId: "acc1",
        threads,
        reload: () => {},
      }),
    );
    act(() => result.current.open());
    expect(result.current.creating).toBe(true);
    expect(result.current.formMailIds).toEqual(["m1", "m2"]);
  });

  it("選択が無ければ open() しても creating=false のまま", () => {
    const { result } = renderHook(() =>
      useCreateProjectFromSelection({
        accountId: "acc1",
        threads: [makeThread("t1", ["m1"])],
        reload: () => {},
      }),
    );
    act(() => result.current.open());
    expect(result.current.creating).toBe(false);
  });

  it("submit() は createProject → bulkMoveMails の順で呼び、選択解除・reload・クローズする", async () => {
    useSelectionStore.setState({ selectedThreadIds: new Set(["t1"]) });
    const threads = [makeThread("t1", ["m1", "m2"])];
    const order: string[] = [];
    const createProject = vi
      .spyOn(useProjectStore.getState(), "createProject")
      .mockImplementation(async () => {
        order.push("create");
        return makeProject("p1");
      });
    const bulkMoveMails = vi
      .spyOn(useMailStore.getState(), "bulkMoveMails")
      .mockImplementation(async () => {
        order.push("move");
        return { succeeded: ["m1", "m2"], failed: [] };
      });
    const reload = vi.fn();

    const { result } = renderHook(() =>
      useCreateProjectFromSelection({ accountId: "acc1", threads, reload }),
    );
    act(() => result.current.open());
    await act(async () => {
      await result.current.submit("新案件", "説明");
    });

    expect(order).toEqual(["create", "move"]);
    expect(createProject).toHaveBeenCalledWith("acc1", "新案件", "説明");
    expect(bulkMoveMails).toHaveBeenCalledWith(["m1", "m2"], "p1");
    expect(reload).toHaveBeenCalled();
    expect(result.current.creating).toBe(false);
    expect(result.current.formMailIds).toEqual([]);
  });

  it("cancel() は creating=false・formMailIds 空に戻す", () => {
    useSelectionStore.setState({ selectedThreadIds: new Set(["t1"]) });
    const { result } = renderHook(() =>
      useCreateProjectFromSelection({
        accountId: "acc1",
        threads: [makeThread("t1", ["m1"])],
        reload: () => {},
      }),
    );
    act(() => result.current.open());
    act(() => result.current.cancel());
    expect(result.current.creating).toBe(false);
    expect(result.current.formMailIds).toEqual([]);
  });
});
