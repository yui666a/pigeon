import { describe, it, expect } from "vitest";
import { htmlToPlain } from "../../utils/htmlToPlain";

describe("htmlToPlain", () => {
  it("turns paragraphs into newline-separated lines", () => {
    expect(htmlToPlain("<p>Hello</p><p>World</p>")).toBe("Hello\nWorld");
  });

  it("preserves multibyte UTF-8 (Japanese) without mojibake", () => {
    expect(htmlToPlain("<p>こんにちは</p><p>世界</p>")).toBe("こんにちは\n世界");
    expect(htmlToPlain("<p>日本語の<strong>本文</strong>です</p>")).toBe(
      "日本語の本文です",
    );
  });

  it("prefixes list items with '- '", () => {
    expect(htmlToPlain("<ul><li>one</li><li>two</li></ul>")).toBe("- one\n- two");
  });

  it("strips inline tags and keeps their text", () => {
    expect(
      htmlToPlain("<p><strong>bold</strong> and <em>italic</em></p>"),
    ).toBe("bold and italic");
  });

  it("collapses excessive blank lines and trims", () => {
    expect(htmlToPlain("<p>a</p><p></p><p></p><p>b</p>")).toBe("a\n\nb");
  });
});
