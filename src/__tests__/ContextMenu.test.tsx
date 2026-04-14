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
