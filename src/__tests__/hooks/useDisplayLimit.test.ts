import { renderHook, act } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { useDisplayLimit } from "../../hooks/useDisplayLimit";

const items = (n: number) => Array.from({ length: n }, (_, i) => i);

describe("useDisplayLimit", () => {
  it("shows at most 200 items initially", () => {
    const { result } = renderHook(() => useDisplayLimit(items(250), "a"));
    expect(result.current.visible).toHaveLength(200);
    expect(result.current.hasMore).toBe(true);
    expect(result.current.remaining).toBe(50);
  });

  it("shows all items when 200 or fewer", () => {
    const { result } = renderHook(() => useDisplayLimit(items(200), "a"));
    expect(result.current.visible).toHaveLength(200);
    expect(result.current.hasMore).toBe(false);
  });

  it("showMore reveals 200 more items", () => {
    const { result } = renderHook(() => useDisplayLimit(items(450), "a"));
    act(() => result.current.showMore());
    expect(result.current.visible).toHaveLength(400);
    act(() => result.current.showMore());
    expect(result.current.visible).toHaveLength(450);
    expect(result.current.hasMore).toBe(false);
  });

  it("resets to 200 when resetKey changes", () => {
    const { result, rerender } = renderHook(
      ({ key }) => useDisplayLimit(items(450), key),
      { initialProps: { key: "a" } },
    );
    act(() => result.current.showMore());
    expect(result.current.visible).toHaveLength(400);
    rerender({ key: "b" });
    expect(result.current.visible).toHaveLength(200);
  });
});
