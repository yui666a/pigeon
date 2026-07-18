import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { MergeProjectDialog } from "../components/sidebar/MergeProjectDialog";
import type { Project } from "../types/project";

const p = (id: string, name: string): Project => ({
  id,
  account_id: "acc1",
  name,
  description: null,
  color: null,
  is_archived: false,
  parent_id: null,
  created_at: "2026-07-18",
  updated_at: "2026-07-18",
});

describe("MergeProjectDialog", () => {
  it("マージ先を選んでボタンを連打してもマージは1回しか実行されない", async () => {
    let resolveMerge!: () => void;
    const onMerge = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveMerge = resolve;
        }),
    );
    render(
      <MergeProjectDialog
        sourceProject={p("src", "元案件")}
        candidates={[p("target", "マージ先案件")]}
        onMerge={onMerge}
        onCancel={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByText("マージ先案件"));
    const mergeButton = screen.getByText("マージ");
    fireEvent.click(mergeButton);
    fireEvent.click(mergeButton);
    fireEvent.click(mergeButton);

    expect(onMerge).toHaveBeenCalledTimes(1);
    resolveMerge();
  });
});
