# Phase 3 テスト補充 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Phase 3 で追加されたフロントエンドコードにユニットテストを補充する

**Architecture:** 既存テストパターン（Vitest + RTL + Zustand setState mock）に従い、新規ストア・コンポーネントのテストファイルを作成する。

**Tech Stack:** Vitest, React Testing Library, Zustand

---

## File Structure

| File | Action | What |
|------|--------|------|
| `src/__tests__/stores/dragStore.test.ts` | Create | dragStore の状態管理テスト |
| `src/__tests__/stores/classifyStore.test.ts` | Modify | moveMail メソッドのテスト追加 |
| `src/__tests__/ContextMenu.test.tsx` | Create | ContextMenu コンポーネントテスト |
| `src/__tests__/DragOverlay.test.tsx` | Create | DragOverlay コンポーネントテスト |

---

## Task 1: dragStore テスト

**Files:**
- Create: `src/__tests__/stores/dragStore.test.ts`

- [ ] **Step 1: Write the test file**

```ts
import { describe, it, expect, beforeEach } from "vitest";
import { useDragStore } from "../../stores/dragStore";

describe("dragStore", () => {
  beforeEach(() => {
    useDragStore.setState({
      draggingMailIds: null,
      mouseX: 0,
      mouseY: 0,
      dragLabel: "",
    });
  });

  describe("startDrag", () => {
    it("sets draggingMailIds and dragLabel", () => {
      useDragStore.getState().startDrag(["m1", "m2"], "Test Subject");

      expect(useDragStore.getState().draggingMailIds).toEqual(["m1", "m2"]);
      expect(useDragStore.getState().dragLabel).toBe("Test Subject");
    });
  });

  describe("updatePosition", () => {
    it("sets mouseX and mouseY", () => {
      useDragStore.getState().updatePosition(100, 200);

      expect(useDragStore.getState().mouseX).toBe(100);
      expect(useDragStore.getState().mouseY).toBe(200);
    });
  });

  describe("endDrag", () => {
    it("clears draggingMailIds and dragLabel", () => {
      useDragStore.getState().startDrag(["m1"], "Subject");
      useDragStore.getState().updatePosition(50, 60);

      useDragStore.getState().endDrag();

      expect(useDragStore.getState().draggingMailIds).toBeNull();
      expect(useDragStore.getState().dragLabel).toBe("");
    });
  });

  describe("full drag cycle", () => {
    it("start → update → end", () => {
      const store = useDragStore.getState();

      store.startDrag(["m1"], "件名");
      expect(useDragStore.getState().draggingMailIds).toEqual(["m1"]);

      store.updatePosition(300, 400);
      expect(useDragStore.getState().mouseX).toBe(300);

      store.endDrag();
      expect(useDragStore.getState().draggingMailIds).toBeNull();
    });
  });
});
```

- [ ] **Step 2: Run test to verify it passes**

Run: `pnpm test -- --run src/__tests__/stores/dragStore.test.ts`
Expected: 4 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/__tests__/stores/dragStore.test.ts
git commit -m "test(stores): dragStore の状態管理テスト追加"
```

---

## Task 2: classifyStore.moveMail テスト追加

**Files:**
- Modify: `src/__tests__/stores/classifyStore.test.ts`

- [ ] **Step 1: Add moveMail tests to existing test file**

既存の `describe("classifyAll")` ブロックの後に追加:

```ts
  describe("moveMail", () => {
    it("calls move_mail and removes mail from unclassified and results", async () => {
      useClassifyStore.setState({
        unclassifiedMails: [
          { id: "m1" } as never,
          { id: "m2" } as never,
        ],
        results: [
          { mail_id: "m1", action: "assign", confidence: 0.9, reason: "test" },
        ],
      });
      mockInvoke.mockResolvedValue(undefined);

      await useClassifyStore.getState().moveMail("m1", "proj1", "acc1");

      expect(mockInvoke).toHaveBeenCalledWith("move_mail", { mailId: "m1", projectId: "proj1" });
      expect(useClassifyStore.getState().unclassifiedMails).toHaveLength(1);
      expect(useClassifyStore.getState().unclassifiedMails[0].id).toBe("m2");
      expect(useClassifyStore.getState().results).toHaveLength(0);
    });

    it("sets error on failure", async () => {
      mockInvoke.mockRejectedValue("move error");

      await useClassifyStore.getState().moveMail("m1", "proj1", "acc1");

      expect(useClassifyStore.getState().error).toBe("move error");
    });
  });
```

- [ ] **Step 2: Run test to verify it passes**

Run: `pnpm test -- --run src/__tests__/stores/classifyStore.test.ts`
Expected: 10 tests PASS (8 existing + 2 new)

- [ ] **Step 3: Commit**

```bash
git add src/__tests__/stores/classifyStore.test.ts
git commit -m "test(stores): classifyStore.moveMail のテスト追加"
```

---

## Task 3: ContextMenu テスト

**Files:**
- Create: `src/__tests__/ContextMenu.test.tsx`

- [ ] **Step 1: Write the test file**

```tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ContextMenu } from "../components/common/ContextMenu";

describe("ContextMenu", () => {
  const defaultItems = [
    { label: "名前変更", onClick: vi.fn() },
    { label: "アーカイブ", onClick: vi.fn() },
    { label: "削除", onClick: vi.fn(), danger: true },
  ];

  it("renders all menu items", () => {
    render(
      <ContextMenu x={100} y={200} items={defaultItems} onClose={vi.fn()} />,
    );
    expect(screen.getByText("名前変更")).toBeInTheDocument();
    expect(screen.getByText("アーカイブ")).toBeInTheDocument();
    expect(screen.getByText("削除")).toBeInTheDocument();
  });

  it("positions at given coordinates", () => {
    const { container } = render(
      <ContextMenu x={100} y={200} items={defaultItems} onClose={vi.fn()} />,
    );
    const menu = container.firstElementChild as HTMLElement;
    expect(menu.style.top).toBe("200px");
    expect(menu.style.left).toBe("100px");
  });

  it("calls item onClick and onClose when item is clicked", () => {
    const onClick = vi.fn();
    const onClose = vi.fn();
    const items = [{ label: "アクション", onClick }];
    render(<ContextMenu x={0} y={0} items={items} onClose={onClose} />);

    fireEvent.click(screen.getByText("アクション"));

    expect(onClick).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("applies danger style to danger items", () => {
    render(
      <ContextMenu x={0} y={0} items={defaultItems} onClose={vi.fn()} />,
    );
    const deleteButton = screen.getByText("削除");
    expect(deleteButton.className).toContain("text-red-600");
  });

  it("does not apply danger style to normal items", () => {
    render(
      <ContextMenu x={0} y={0} items={defaultItems} onClose={vi.fn()} />,
    );
    const renameButton = screen.getByText("名前変更");
    expect(renameButton.className).not.toContain("text-red-600");
  });

  it("calls onClose when clicking outside", () => {
    const onClose = vi.fn();
    render(
      <div>
        <span data-testid="outside">outside</span>
        <ContextMenu x={0} y={0} items={defaultItems} onClose={onClose} />
      </div>,
    );

    fireEvent.mouseDown(screen.getByTestId("outside"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("calls onClose when Escape is pressed", () => {
    const onClose = vi.fn();
    render(
      <ContextMenu x={0} y={0} items={defaultItems} onClose={onClose} />,
    );

    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
```

- [ ] **Step 2: Run test to verify it passes**

Run: `pnpm test -- --run src/__tests__/ContextMenu.test.tsx`
Expected: 7 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/__tests__/ContextMenu.test.tsx
git commit -m "test(ui): ContextMenu のレンダリング・操作テスト追加"
```

---

## Task 4: DragOverlay テスト

**Files:**
- Create: `src/__tests__/DragOverlay.test.tsx`

- [ ] **Step 1: Write the test file**

```tsx
import { render, screen } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import { DragOverlay } from "../components/common/DragOverlay";
import { useDragStore } from "../stores/dragStore";

describe("DragOverlay", () => {
  beforeEach(() => {
    useDragStore.setState({
      draggingMailIds: null,
      mouseX: 0,
      mouseY: 0,
      dragLabel: "",
    });
  });

  it("renders nothing when not dragging", () => {
    const { container } = render(<DragOverlay />);
    expect(container.firstChild).toBeNull();
  });

  it("renders drag label when dragging", () => {
    useDragStore.setState({
      draggingMailIds: ["m1"],
      mouseX: 100,
      mouseY: 200,
      dragLabel: "テストメール",
    });

    render(<DragOverlay />);
    expect(screen.getByText("テストメール")).toBeInTheDocument();
  });

  it("shows count badge for multiple mails", () => {
    useDragStore.setState({
      draggingMailIds: ["m1", "m2", "m3"],
      mouseX: 100,
      mouseY: 200,
      dragLabel: "テストメール",
    });

    render(<DragOverlay />);
    expect(screen.getByText("3")).toBeInTheDocument();
  });

  it("does not show count badge for single mail", () => {
    useDragStore.setState({
      draggingMailIds: ["m1"],
      mouseX: 100,
      mouseY: 200,
      dragLabel: "テストメール",
    });

    render(<DragOverlay />);
    expect(screen.queryByText("1")).not.toBeInTheDocument();
  });

  it("positions at mouse coordinates with offset", () => {
    useDragStore.setState({
      draggingMailIds: ["m1"],
      mouseX: 150,
      mouseY: 250,
      dragLabel: "テスト",
    });

    const { container } = render(<DragOverlay />);
    const overlay = container.firstElementChild as HTMLElement;
    expect(overlay.style.top).toBe("262px");
    expect(overlay.style.left).toBe("162px");
  });
});
```

- [ ] **Step 2: Run test to verify it passes**

Run: `pnpm test -- --run src/__tests__/DragOverlay.test.tsx`
Expected: 5 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/__tests__/DragOverlay.test.tsx
git commit -m "test(ui): DragOverlay のレンダリングテスト追加"
```

---

## Summary

| Task | Module | New Tests |
|------|--------|-----------|
| 1 | dragStore | 4 |
| 2 | classifyStore.moveMail | 2 |
| 3 | ContextMenu | 7 |
| 4 | DragOverlay | 5 |
| **Total** | | **18** |

Before: 132 Rust tests + 71 frontend tests (203 total)
After: 132 Rust tests + 89 frontend tests (221 total)
