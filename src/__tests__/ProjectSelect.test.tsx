import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ProjectSelect } from "../components/common/ProjectSelect";
import type { Project } from "../types/project";

function makeProject(id: string, name: string, parentId?: string): Project {
  return {
    id,
    account_id: "acc1",
    name,
    description: null,
    color: null,
    is_archived: false,
    parent_id: parentId ?? null,
    created_at: "2026-07-13T00:00:00",
    updated_at: "2026-07-13T00:00:00",
  };
}

describe("ProjectSelect", () => {
  it("shows each project as its full path", () => {
    const projects = [makeProject("p1", "親"), makeProject("p2", "子", "p1")];
    render(
      <ProjectSelect
        projects={projects}
        ariaLabel="案件へ移動"
        placeholder="案件へ移動..."
        onSelect={() => {}}
      />,
    );

    expect(screen.getByRole("option", { name: "親 > 子" })).toBeInTheDocument();
  });

  it("orders options by path so siblings group under their parent", () => {
    // 挿入順をパス順とわざと変えて、パス順に並ぶことを見る
    const projects = [
      makeProject("p2", "B"),
      makeProject("p1", "A"),
      makeProject("p3", "A子", "p1"),
    ];
    render(
      <ProjectSelect
        projects={projects}
        ariaLabel="案件へ移動"
        placeholder="案件へ移動..."
        onSelect={() => {}}
      />,
    );

    const labels = screen
      .getAllByRole("option")
      .map((o) => o.textContent)
      .filter((t) => t !== "案件へ移動...");
    expect(labels).toEqual(["A", "A > A子", "B"]);
  });

  it("reports the selected project id", () => {
    const onSelect = vi.fn();
    render(
      <ProjectSelect
        projects={[makeProject("p1", "案件A")]}
        ariaLabel="案件へ移動"
        placeholder="案件へ移動..."
        onSelect={onSelect}
      />,
    );

    fireEvent.change(screen.getByLabelText("案件へ移動"), {
      target: { value: "p1" },
    });

    expect(onSelect).toHaveBeenCalledWith("p1");
  });

  it("resets to the placeholder after selecting so the same project can be chosen again", () => {
    const onSelect = vi.fn();
    render(
      <ProjectSelect
        projects={[makeProject("p1", "案件A")]}
        ariaLabel="案件へ移動"
        placeholder="案件へ移動..."
        onSelect={onSelect}
      />,
    );

    const select = screen.getByLabelText<HTMLSelectElement>("案件へ移動");
    fireEvent.change(select, { target: { value: "p1" } });
    expect(select.value).toBe("");

    fireEvent.change(select, { target: { value: "p1" } });
    expect(onSelect).toHaveBeenCalledTimes(2);
  });

  it("does not fire onSelect when the placeholder itself is chosen", () => {
    const onSelect = vi.fn();
    render(
      <ProjectSelect
        projects={[makeProject("p1", "案件A")]}
        ariaLabel="案件へ移動"
        placeholder="案件へ移動..."
        onSelect={onSelect}
      />,
    );

    fireEvent.change(screen.getByLabelText("案件へ移動"), {
      target: { value: "" },
    });

    expect(onSelect).not.toHaveBeenCalled();
  });

  it("keeps the selection visible when value is controlled", () => {
    // 「修正する」UI のように選択を保持したいケース
    render(
      <ProjectSelect
        projects={[makeProject("p1", "案件A")]}
        ariaLabel="移動先"
        placeholder="選択してください"
        value="p1"
        onSelect={() => {}}
      />,
    );

    expect(screen.getByLabelText<HTMLSelectElement>("移動先").value).toBe("p1");
  });
});
