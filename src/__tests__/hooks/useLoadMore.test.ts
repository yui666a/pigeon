import { renderHook, act, waitFor } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { useLoadMore } from "../../hooks/useLoadMore";

describe("useLoadMore", () => {
  it("後続があるときだけ「もっと見る」を出す", () => {
    const { result } = renderHook(() => useLoadMore(true, vi.fn()));
    expect(result.current.hasMore).toBe(true);

    const { result: none } = renderHook(() => useLoadMore(false, vi.fn()));
    expect(none.current.hasMore).toBe(false);
  });

  it("loadMore で追加取得を呼ぶ", async () => {
    const fetchMore = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() => useLoadMore(true, fetchMore));

    await act(async () => {
      await result.current.loadMore();
    });

    expect(fetchMore).toHaveBeenCalledTimes(1);
  });

  it("取得中は loading になり、二重取得しない", async () => {
    let resolve: (() => void) | undefined;
    const fetchMore = vi.fn(
      () =>
        new Promise<void>((r) => {
          resolve = r;
        }),
    );
    const { result } = renderHook(() => useLoadMore(true, fetchMore));

    act(() => {
      void result.current.loadMore();
    });
    await waitFor(() => expect(result.current.loading).toBe(true));

    // 取得中の追加呼び出しは無視される
    act(() => {
      void result.current.loadMore();
    });
    expect(fetchMore).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolve?.();
    });
    await waitFor(() => expect(result.current.loading).toBe(false));
  });

  it("後続が無ければ loadMore は何もしない", async () => {
    const fetchMore = vi.fn();
    const { result } = renderHook(() => useLoadMore(false, fetchMore));

    await act(async () => {
      await result.current.loadMore();
    });

    expect(fetchMore).not.toHaveBeenCalled();
  });

  it("取得が失敗しても loading は解除される", async () => {
    const fetchMore = vi.fn().mockRejectedValue(new Error("boom"));
    const { result } = renderHook(() => useLoadMore(true, fetchMore));

    await act(async () => {
      await result.current.loadMore();
    });

    await waitFor(() => expect(result.current.loading).toBe(false));
  });
});
