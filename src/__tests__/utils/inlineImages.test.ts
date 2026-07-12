import { describe, it, expect } from "vitest";
import { replaceCidReferences, hasCidReferences } from "../../utils/inlineImages";
import type { InlineImage } from "../../types/attachment";

describe("hasCidReferences", () => {
  it("cid: 参照を含むimgタグがあれば true", () => {
    expect(hasCidReferences('<img src="cid:logo123@example.com">')).toBe(true);
  });

  it("cid: 参照がなければ false", () => {
    expect(hasCidReferences('<img src="https://example.com/a.png">')).toBe(false);
    expect(hasCidReferences("<p>本文のみ</p>")).toBe(false);
  });
});

describe("replaceCidReferences", () => {
  it("対応するcontent_idの画像があればdata URIに置換する", () => {
    const html = '<p>見て<img src="cid:logo123@example.com" alt="logo"></p>';
    const images: InlineImage[] = [
      { content_id: "logo123@example.com", data_uri: "data:image/png;base64,AAAA" },
    ];
    const result = replaceCidReferences(html, images);
    expect(result).toContain('src="data:image/png;base64,AAAA"');
    expect(result).not.toContain("cid:logo123");
    // 他の属性は保持される
    expect(result).toContain('alt="logo"');
  });

  it("対応する画像が見つからないcid参照はそのまま残す", () => {
    const html = '<img src="cid:unknown@example.com">';
    const result = replaceCidReferences(html, []);
    expect(result).toContain("cid:unknown@example.com");
  });

  it("cid以外のsrc（外部URL）は変更しない", () => {
    const html = '<img src="https://example.com/tracker.png">';
    const images: InlineImage[] = [
      { content_id: "logo123@example.com", data_uri: "data:image/png;base64,AAAA" },
    ];
    const result = replaceCidReferences(html, images);
    expect(result).toContain('src="https://example.com/tracker.png"');
  });

  it("cid参照がない本文はそのまま返す", () => {
    const html = "<p>本文のみ</p>";
    expect(replaceCidReferences(html, [])).toBe(html);
  });

  it("複数のcid画像をそれぞれ正しく置換する", () => {
    const html =
      '<img src="cid:a@example.com"><img src="cid:b@example.com">';
    const images: InlineImage[] = [
      { content_id: "a@example.com", data_uri: "data:image/png;base64,AAA" },
      { content_id: "b@example.com", data_uri: "data:image/jpeg;base64,BBB" },
    ];
    const result = replaceCidReferences(html, images);
    expect(result).toContain('src="data:image/png;base64,AAA"');
    expect(result).toContain('src="data:image/jpeg;base64,BBB"');
  });
});
