import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ProjectPanel } from "./ProjectPanel";
import type { MapProject } from "../../types/embeddingMap";

const projects: MapProject[] = [
  { id: "p1", name: "案件A", color: "#ff0000" },
  { id: "p2", name: "案件B", color: null },
];

describe("ProjectPanel", () => {
  it("案件名を一覧表示する", () => {
    render(<ProjectPanel projects={projects} dropActive={false} onDrop={vi.fn()} />);
    expect(screen.getByText("案件A")).toBeInTheDocument();
    expect(screen.getByText("案件B")).toBeInTheDocument();
  });

  it("ドラッグ中に mouseup した案件を onDrop へ渡す", () => {
    const onDrop = vi.fn();
    render(<ProjectPanel projects={projects} dropActive={true} onDrop={onDrop} />);
    fireEvent.mouseUp(screen.getByText("案件A"));
    expect(onDrop).toHaveBeenCalledWith(projects[0]);
  });

  it("ドラッグ中でなければ mouseup しても onDrop を呼ばない", () => {
    const onDrop = vi.fn();
    render(<ProjectPanel projects={projects} dropActive={false} onDrop={onDrop} />);
    fireEvent.mouseUp(screen.getByText("案件A"));
    expect(onDrop).not.toHaveBeenCalled();
  });
});
