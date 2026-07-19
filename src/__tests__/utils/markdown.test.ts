import { describe, it, expect } from "vitest";
import { Editor } from "@tiptap/core";
import { NOTE_EXTENSIONS, getMarkdownStorage } from "../../utils/markdown";

function makeEditor(md: string): Editor {
  const editor = new Editor({ extensions: NOTE_EXTENSIONS, content: "" });
  editor.commands.setContent(md);
  return editor;
}

function roundtrip(md: string): string {
  const editor = makeEditor(md);
  // tiptap-markdown が提供する storage 経由で Markdown を読み書きする
  const out = getMarkdownStorage(editor).getMarkdown();
  editor.destroy();
  return out;
}

describe("markdown roundtrip", () => {
  it("見出しと強調を保持する", () => {
    const out = roundtrip("# 春公演\n\n**会場担当**: 伊藤\n");
    expect(out).toContain("# 春公演");
    expect(out).toContain("**会場担当**");
  });

  it("箇条書きを保持する", () => {
    const out = roundtrip("- 搬入 9:00\n- リハ 13:00\n");
    expect(out).toContain("- 搬入 9:00");
    expect(out).toContain("- リハ 13:00");
  });

  it("表を保持する", () => {
    const md = "| 時刻 | 内容 |\n| --- | --- |\n| 9:00 | 搬入 |\n";

    // 文字列の部分一致だけでは、表がプレーンテキストに潰れても偶然一致しうる。
    // ここでは (1) パース結果の ProseMirror ドキュメントに table ノードが
    // 実在すること、(2) 出力 Markdown が GFM 表の区切り行を含むこと の両方を検証する。
    const editor = makeEditor(md);
    const tableNodeCount = editor.$nodes("table")?.length ?? 0;
    expect(tableNodeCount).toBe(1);

    const out = getMarkdownStorage(editor).getMarkdown();
    editor.destroy();

    expect(out).toContain("9:00");
    expect(out).toContain("搬入");
    // GFM のヘッダー区切り行（例: `| --- | --- |`）が含まれることを確認する
    expect(out).toMatch(/\|\s*-{3,}\s*\|/);
    // 見出しセルも保持されていること
    expect(out).toContain("時刻");
    expect(out).toContain("内容");
  });

  it("空文字を扱える", () => {
    expect(roundtrip("")).toBe("");
  });
});
