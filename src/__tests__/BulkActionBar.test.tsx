import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { BulkActionBar } from "../components/thread-list/BulkActionBar";
import type { Project } from "../types/project";

function makeProject(id: string, name: string): Project {
  return {
    id,
    account_id: "acc1",
    name,
    description: null,
    color: null,
    is_archived: false,
    parent_id: null,
    created_at: "2026-07-13T00:00:00",
    updated_at: "2026-07-13T00:00:00",
  };
}

describe("BulkActionBar", () => {
  it("renders nothing when no thread is selected", () => {
    const { container } = render(
      <BulkActionBar
        selectedCount={0}
        projects={[]}
        onDelete={() => {}}
        onArchive={() => {}}
        onMove={() => {}}
        onClear={() => {}}
        onCreateProject={() => {}}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the selected count", () => {
    render(
      <BulkActionBar
        selectedCount={3}
        projects={[]}
        onDelete={() => {}}
        onArchive={() => {}}
        onMove={() => {}}
        onClear={() => {}}
        onCreateProject={() => {}}
      />,
    );
    expect(screen.getByText("3 件選択中")).toBeInTheDocument();
  });

  it("calls onDelete/onArchive/onClear when the respective buttons are clicked", () => {
    const onDelete = vi.fn();
    const onArchive = vi.fn();
    const onClear = vi.fn();
    render(
      <BulkActionBar
        selectedCount={2}
        projects={[]}
        onDelete={onDelete}
        onArchive={onArchive}
        onMove={() => {}}
        onClear={onClear}
        onCreateProject={() => {}}
      />,
    );
    fireEvent.click(screen.getByText("削除"));
    fireEvent.click(screen.getByText("アーカイブ"));
    fireEvent.click(screen.getByText("選択解除"));
    expect(onDelete).toHaveBeenCalledTimes(1);
    expect(onArchive).toHaveBeenCalledTimes(1);
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it("calls onMove with the selected project id", () => {
    const onMove = vi.fn();
    render(
      <BulkActionBar
        selectedCount={1}
        projects={[makeProject("p1", "Project A")]}
        onDelete={() => {}}
        onArchive={() => {}}
        onMove={onMove}
        onClear={() => {}}
        onCreateProject={() => {}}
      />,
    );
    fireEvent.change(screen.getByLabelText("案件へ移動"), {
      target: { value: "p1" },
    });
    expect(onMove).toHaveBeenCalledWith("p1");
  });

  it("「＋ 新しい案件」ボタンで onCreateProject を発火する", () => {
    const onCreateProject = vi.fn();
    render(
      <BulkActionBar
        selectedCount={3}
        projects={[]}
        onDelete={() => {}}
        onArchive={() => {}}
        onMove={() => {}}
        onClear={() => {}}
        onCreateProject={onCreateProject}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /新しい案件/ }));
    expect(onCreateProject).toHaveBeenCalledTimes(1);
  });
});

describe("BulkActionBar (階層案件)", () => {
  it("案件の選択肢はパス表記でパス順に並ぶ", () => {
    const tour = makeProject("tour", "ツアー");
    const venue = { ...makeProject("venue", "埼玉"), parent_id: "tour" };
    const other = makeProject("other", "別件");
    render(
      <BulkActionBar
        selectedCount={1}
        projects={[other, venue, tour]}
        onDelete={() => {}}
        onArchive={() => {}}
        onMove={() => {}}
        onClear={() => {}}
        onCreateProject={() => {}}
      />,
    );
    const labels = screen
      .getAllByRole("option")
      .filter((o) => !(o as HTMLOptionElement).disabled)
      .map((o) => o.textContent);
    expect(labels).toEqual(["ツアー", "ツアー > 埼玉", "別件"]);
  });
});
