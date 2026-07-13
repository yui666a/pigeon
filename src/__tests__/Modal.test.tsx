import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { Modal } from "../components/common/Modal";

describe("Modal", () => {
  it("children をダイアログとして描画する", () => {
    render(
      <Modal ariaLabel="テスト" onClose={() => {}}>
        <p>本文です</p>
      </Modal>,
    );
    expect(screen.getByText("本文です")).toBeInTheDocument();
  });

  it("role=dialog と aria-modal / aria-label が付く", () => {
    render(
      <Modal ariaLabel="テスト設定" onClose={() => {}}>
        <p>本文</p>
      </Modal>,
    );
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveAttribute("aria-label", "テスト設定");
  });

  it("Escape キーで onClose が呼ばれる", () => {
    const onClose = vi.fn();
    render(
      <Modal ariaLabel="テスト" onClose={onClose}>
        <p>本文</p>
      </Modal>,
    );
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("Escape 以外のキーでは onClose は呼ばれない", () => {
    const onClose = vi.fn();
    render(
      <Modal ariaLabel="テスト" onClose={onClose}>
        <p>本文</p>
      </Modal>,
    );
    fireEvent.keyDown(document, { key: "Enter" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("アンマウント後は Escape キーで onClose が呼ばれない", () => {
    const onClose = vi.fn();
    const { unmount } = render(
      <Modal ariaLabel="テスト" onClose={onClose}>
        <p>本文</p>
      </Modal>,
    );
    unmount();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("オーバーレイやパネルのクリックでは閉じない（既存3ダイアログの挙動に統一）", () => {
    const onClose = vi.fn();
    render(
      <Modal ariaLabel="テスト" onClose={onClose}>
        <p>本文</p>
      </Modal>,
    );
    fireEvent.click(screen.getByRole("dialog").parentElement as HTMLElement);
    fireEvent.click(screen.getByText("本文"));
    expect(onClose).not.toHaveBeenCalled();
  });

  it("className でパネルの幅などを拡張できる", () => {
    render(
      <Modal ariaLabel="テスト" onClose={() => {}} className="w-80 p-4">
        <p>本文</p>
      </Modal>,
    );
    const dialog = screen.getByRole("dialog");
    expect(dialog.className).toContain("w-80");
    expect(dialog.className).toContain("p-4");
  });
});
