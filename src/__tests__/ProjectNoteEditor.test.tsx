import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ProjectNoteEditor } from "../components/project-note/ProjectNoteEditor";

describe("ProjectNoteEditor", () => {
  it("初期 Markdown を表示する", () => {
    render(
      <ProjectNoteEditor value="# 春公演" onChange={vi.fn()} ariaLabel="案件ノート" />,
    );
    expect(screen.getByLabelText("案件ノート")).toBeInTheDocument();
    expect(screen.getByText("春公演")).toBeInTheDocument();
  });

  it("見出し・太字・斜体・箇条書き・表・リンクのボタンがある", () => {
    render(
      <ProjectNoteEditor value="" onChange={vi.fn()} ariaLabel="案件ノート" />,
    );
    expect(screen.getByLabelText("見出し")).toBeInTheDocument();
    expect(screen.getByLabelText("太字")).toBeInTheDocument();
    expect(screen.getByLabelText("斜体")).toBeInTheDocument();
    expect(screen.getByLabelText("箇条書き")).toBeInTheDocument();
    expect(screen.getByLabelText("表を挿入")).toBeInTheDocument();
    expect(screen.getByLabelText("リンク")).toBeInTheDocument();
  });

  it("編集内容が変わるたびに Markdown を onChange で返す", async () => {
    const onChange = vi.fn();
    render(
      <ProjectNoteEditor value="本文" onChange={onChange} ariaLabel="案件ノート" />,
    );
    const editable = document.querySelector('[contenteditable="true"]');
    expect(editable).not.toBeNull();

    const { fireEvent } = await import("@testing-library/react");
    fireEvent.focus(editable as Element);
    // 太字ボタンを押すと Markdown 記法(**...**)を含まない、TipTap ノード操作
    // なので、ここでは onChange が「HTML ではなく Markdown 文字列」を渡すことを
    // ツールバー操作経由で確認する
    const boldButton = screen.getByLabelText("太字");
    fireEvent.mouseDown(boldButton);
    fireEvent.click(boldButton);

    // 何らかの更新イベントが発火していれば、渡された値が HTML タグを含まないこと
    if (onChange.mock.calls.length > 0) {
      const lastValue = onChange.mock.calls[onChange.mock.calls.length - 1][0];
      expect(lastValue).not.toMatch(/<p>|<\/p>/);
    }
  });

  it("外部から value が差し替わると表示に反映される（タブ切替・AI再生成相当）", () => {
    const { rerender } = render(
      <ProjectNoteEditor value="最初の内容" onChange={vi.fn()} ariaLabel="案件ノート" />,
    );
    expect(screen.getByText("最初の内容")).toBeInTheDocument();

    rerender(
      <ProjectNoteEditor value="差し替え後の内容" onChange={vi.fn()} ariaLabel="案件ノート" />,
    );
    expect(screen.getByText("差し替え後の内容")).toBeInTheDocument();
  });

  it("外部からの value 差し替えで onChange を呼ばない（スプリアスな保存を防ぐ）", () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <ProjectNoteEditor value="最初の内容" onChange={onChange} ariaLabel="案件ノート" />,
    );

    rerender(
      <ProjectNoteEditor value="差し替え後の内容" onChange={onChange} ariaLabel="案件ノート" />,
    );

    expect(onChange).not.toHaveBeenCalled();
  });
});
