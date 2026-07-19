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
        projects={[p("src", "元案件"), p("target", "マージ先案件")]}
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

describe("MergeProjectDialog (階層案件)", () => {
  it("候補はパス表記でパス順に表示される", () => {
    const tour = p("tour", "ツアー");
    const venue = { ...p("venue", "埼玉"), parent_id: "tour" };
    const other = p("other", "別件");
    const src = p("src", "元案件");
    render(
      <MergeProjectDialog
        sourceProject={src}
        candidates={[other, venue, tour]}
        projects={[src, other, venue, tour]}
        onMerge={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(screen.getByText("ツアー > 埼玉")).toBeInTheDocument();
    const buttons = [
      screen.getByText("ツアー"),
      screen.getByText("ツアー > 埼玉"),
      screen.getByText("別件"),
    ];
    // DOM順がパス順になっている
    expect(
      buttons[0].compareDocumentPosition(buttons[1]) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      buttons[1].compareDocumentPosition(buttons[2]) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });
});
