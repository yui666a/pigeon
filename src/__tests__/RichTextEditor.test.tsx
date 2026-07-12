import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { RichTextEditor } from "../components/compose/RichTextEditor";

describe("RichTextEditor", () => {
  it("renders the minimal formatting toolbar", () => {
    render(<RichTextEditor value="<p>hi</p>" onChange={() => {}} />);
    expect(screen.getByRole("button", { name: "太字" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "斜体" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "箇条書き" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "リンク" })).toBeInTheDocument();
  });

  it("renders the initial HTML content into the editable area", () => {
    render(<RichTextEditor value="<p>初期本文</p>" onChange={() => {}} />);
    expect(screen.getByText("初期本文")).toBeInTheDocument();
  });

  it("syncs the editable region when the value prop changes", () => {
    const { rerender } = render(
      <RichTextEditor value="<p>最初</p>" onChange={() => {}} />,
    );
    expect(screen.getByText("最初")).toBeInTheDocument();
    // フォーマット切替などで body がまるごと差し替わったとき追随する
    rerender(<RichTextEditor value="<p>差し替え後</p>" onChange={() => {}} />);
    expect(screen.getByText("差し替え後")).toBeInTheDocument();
  });

  it("exposes the editable area as a contenteditable region", () => {
    render(<RichTextEditor value="<p>x</p>" onChange={() => {}} />);
    const editable = document.querySelector('[contenteditable="true"]');
    expect(editable).not.toBeNull();
    expect(editable?.getAttribute("aria-label")).toBe("本文（リッチテキスト）");
  });
});
