import { describe, expect, it } from "vitest";
import { buildProjectTree, aggregateUnread, projectPathString } from "../stores/projectTree";
import type { Project } from "../types/project";

const p = (id: string, parent: string | null): Project => ({
  id, account_id: "acc1", name: id, description: null, color: null,
  is_archived: false, parent_id: parent,
  created_at: "2026-07-18", updated_at: "2026-07-18",
});

describe("buildProjectTree", () => {
  it("builds nested tree from flat array", () => {
    const tree = buildProjectTree([p("root", null), p("mid", "root"), p("leaf", "mid"), p("other", null)]);
    expect(tree).toHaveLength(2);
    const root = tree.find((n) => n.project.id === "root")!;
    expect(root.children[0].project.id).toBe("mid");
    expect(root.children[0].children[0].project.id).toBe("leaf");
  });

  it("treats orphan parent_id as root (archived ancestor not in list)", () => {
    const tree = buildProjectTree([p("child", "gone")]);
    expect(tree).toHaveLength(1);
  });
});

describe("aggregateUnread", () => {
  it("sums descendants bottom-up", () => {
    const projects = [p("root", null), p("mid", "root"), p("leaf", "mid")];
    const agg = aggregateUnread(projects, { root: 1, mid: 2, leaf: 3 });
    expect(agg).toEqual({ root: 6, mid: 5, leaf: 3 });
  });
});

describe("projectPathString", () => {
  it("joins ancestor names root-first with ' > '", () => {
    const projects = [p("root", null), p("mid", "root"), p("leaf", "mid")];
    expect(projectPathString(projects, "leaf")).toBe("root > mid > leaf");
  });

  it("returns just the name for a root project", () => {
    const projects = [p("root", null)];
    expect(projectPathString(projects, "root")).toBe("root");
  });

  it("returns empty string for an unknown id", () => {
    const projects = [p("root", null)];
    expect(projectPathString(projects, "missing")).toBe("");
  });

  it("stops at an orphan parent_id not present in the array", () => {
    const projects = [p("child", "gone")];
    expect(projectPathString(projects, "child")).toBe("child");
  });
});
