import { describe, it, expect } from "vitest";
import { buildMailFrameSrcdoc } from "../utils/buildMailFrameSrcdoc";

describe("buildMailFrameSrcdoc", () => {
  it("サニタイズ済みHTMLをそのままbodyに埋め込む", () => {
    const html = '<p>こんにちは <strong>world</strong></p>';
    const doc = buildMailFrameSrcdoc(html);
    expect(doc).toContain(html);
    // body 要素内に入っている（先頭でも末尾でもなく文書構造の中）
    expect(doc.indexOf("<body>")).toBeLessThan(doc.indexOf(html));
    expect(doc.indexOf(html)).toBeLessThan(doc.indexOf("</body>"));
  });

  it("文字化け防止のmeta charsetを含む", () => {
    expect(buildMailFrameSrcdoc("<p>x</p>")).toContain('<meta charset="utf-8">');
  });

  it("画像のはみ出しを防ぐ基本スタイルを含む", () => {
    const doc = buildMailFrameSrcdoc("<p>x</p>");
    expect(doc).toMatch(/img\s*\{[^}]*max-width:\s*100%/);
  });

  it("長い英数字文字列の折り返しスタイルを含む", () => {
    const doc = buildMailFrameSrcdoc("<p>x</p>");
    expect(doc).toMatch(/overflow-wrap:\s*anywhere/);
  });

  it("完全なHTML文書として組み立てる", () => {
    const doc = buildMailFrameSrcdoc("<p>x</p>");
    expect(doc.startsWith("<!doctype html>")).toBe(true);
    expect(doc).toContain("</html>");
  });
});
