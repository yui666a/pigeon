import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ProjectListItem } from "../components/sidebar/ProjectListItem";
import { ProjectRenameProvider } from "../components/sidebar/ProjectRenameContext";
import type { Project } from "../types/project";
import type { ProjectDirectory } from "../types/directory";

const project: Project = {
  id: "p1",
  account_id: "acc1",
  name: "春公演",
  description: null,
  color: "#3E617F",
  is_archived: false,
  created_at: "",
  updated_at: "",
};

function renderItem(directory?: ProjectDirectory | null, scanning?: boolean) {
  return render(
    <ProjectRenameProvider projects={[project]}>
      <ul>
        <ProjectListItem
          project={project}
          selected={false}
          onSelect={vi.fn()}
          onContextMenu={vi.fn()}
          onDrop={vi.fn()}
          directory={directory}
          scanning={scanning}
        />
      </ul>
    </ProjectRenameProvider>,
  );
}

describe("ProjectListItem directory indicators", () => {
  it("shows no folder icon when unlinked", () => {
    renderItem(null);
    expect(screen.queryByTitle(/\/tmp/)).not.toBeInTheDocument();
    expect(screen.queryByText("📁")).not.toBeInTheDocument();
  });

  it("shows 📁 when linked and status is ok", () => {
    renderItem({
      id: "d1", project_id: "p1", path: "/tmp/stage-a", is_primary: true,
      status: "ok", last_scanned_at: null, created_at: "",
    });
    expect(screen.getByTitle("/tmp/stage-a")).toHaveTextContent("📁");
  });

  it("shows warning icon when directory is missing", () => {
    renderItem({
      id: "d1", project_id: "p1", path: "/tmp/gone", is_primary: true,
      status: "missing", last_scanned_at: null, created_at: "",
    });
    const badge = screen.getByTitle(/フォルダにアクセスできません/);
    expect(badge).toHaveTextContent("⚠");
  });

  it("shows scanning indicator while rescanning", () => {
    renderItem(
      {
        id: "d1", project_id: "p1", path: "/tmp/stage-a", is_primary: true,
        status: "ok", last_scanned_at: null, created_at: "",
      },
      true,
    );
    expect(screen.getByTitle("スキャン中")).toBeInTheDocument();
  });
});
