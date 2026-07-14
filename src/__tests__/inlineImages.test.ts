import { describe, it, expect } from "vitest";
import { replaceCidReferences, hasCidReferences } from "../utils/inlineImages";

describe("replaceCidReferences", () => {
  it("対応するcid参照をdata URIに置換する", () => {
    const html = '<img src="cid:logo@ex.com" alt="logo">';
    const out = replaceCidReferences(html, [
      { content_id: "logo@ex.com", data_uri: "data:image/png;base64,AAAA" },
    ]);
    expect(out).toContain('src="data:image/png;base64,AAAA"');
  });

  it("対応が見つからないcid参照はそのまま残す", () => {
    const html = '<img src="cid:unknown@ex.com">';
    const out = replaceCidReferences(html, []);
    expect(out).toContain('src="cid:unknown@ex.com"');
  });

  it("画像以外のMIMEのdata URIは差し込まない（防御の多層化）", () => {
    // バックエンドも許可リストで検証するが、フロント側でも data:image/ 以外を拒否する
    const html = '<img src="cid:evil@ex.com">';
    const out = replaceCidReferences(html, [
      { content_id: "evil@ex.com", data_uri: "data:text/html;base64,PHNjcmlwdD4=" },
    ]);
    expect(out).not.toContain("data:text/html");
    expect(out).toContain('src="cid:evil@ex.com"');
  });
});

describe("hasCidReferences", () => {
  it("cid参照を含むimgを検知する", () => {
    expect(hasCidReferences('<img src="cid:a@b">')).toBe(true);
    expect(hasCidReferences("<p>plain</p>")).toBe(false);
  });
});
