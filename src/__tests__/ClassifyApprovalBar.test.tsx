import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ClassifyApprovalBar } from "../components/mail-view/ClassifyApprovalBar";
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
    created_at: "2026-07-19T00:00:00",
    updated_at: "2026-07-19T00:00:00",
  };
}

const projects = [makeProject("p1", "案件A"), makeProject("p2", "案件B")];

describe("ClassifyApprovalBar", () => {
  it("「正しい」で現在の案件のまま承認する", () => {
    const onApprove = vi.fn();
    render(
      <ClassifyApprovalBar
        projects={projects}
        currentProjectId="p1"
        onApprove={onApprove}
        onDismiss={() => {}}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "正しい" }));
    expect(onApprove).toHaveBeenCalledWith("p1");
  });

  it("「修正する」を押すまで移動先セレクトを出さない", () => {
    render(
      <ClassifyApprovalBar
        projects={projects}
        currentProjectId="p1"
        onApprove={() => {}}
        onDismiss={() => {}}
      />,
    );

    expect(screen.queryByLabelText("移動先の案件")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "修正する" }));
    expect(screen.getByLabelText("移動先の案件")).toBeInTheDocument();
  });

  it("修正先を選ぶとその案件で承認する", () => {
    const onApprove = vi.fn();
    render(
      <ClassifyApprovalBar
        projects={projects}
        currentProjectId="p1"
        onApprove={onApprove}
        onDismiss={() => {}}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "修正する" }));
    fireEvent.change(screen.getByLabelText("移動先の案件"), {
      target: { value: "p2" },
    });

    expect(onApprove).toHaveBeenCalledWith("p2");
  });

  it("確定先が分からない場面では最初から移動先を選ばせる", () => {
    // INBOX 一覧など案件を選んでいない場面。空の案件で確定させない
    render(
      <ClassifyApprovalBar
        projects={projects}
        currentProjectId=""
        onApprove={() => {}}
        onDismiss={() => {}}
      />,
    );

    expect(screen.getByLabelText("移動先の案件")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "正しい" }),
    ).not.toBeInTheDocument();
  });

  it("閉じるとバーを消す", () => {
    const onDismiss = vi.fn();
    render(
      <ClassifyApprovalBar
        projects={projects}
        currentProjectId="p1"
        onApprove={() => {}}
        onDismiss={onDismiss}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "閉じる" }));
    expect(onDismiss).toHaveBeenCalled();
  });
});
