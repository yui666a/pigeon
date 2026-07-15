import { describe, it, expect } from "vitest";
import {
  extractExternalImageUrls,
  replaceExternalImageUrls,
} from "../utils/externalImages";

describe("extractExternalImageUrls", () => {
  it("http(s)のimg srcを列挙する", () => {
    const urls = extractExternalImageUrls(
      '<img src="https://ex.com/a.png"><p>x</p><img src="http://ex.com/b.gif">',
    );
    expect(urls).toEqual(["https://ex.com/a.png", "http://ex.com/b.gif"]);
  });

  it("重複URLは1つにまとめる", () => {
    const urls = extractExternalImageUrls(
      '<img src="https://ex.com/a.png"><img src="https://ex.com/a.png">',
    );
    expect(urls).toEqual(["https://ex.com/a.png"]);
  });

  it("data:/cid:/プロトコル相対/相対URLは対象外", () => {
    const urls = extractExternalImageUrls(
      '<img src="data:image/png;base64,AA"><img src="cid:logo@ex.com">' +
        '<img src="//ex.com/p.gif"><img src="/local.png"><img alt="no-src">',
    );
    expect(urls).toEqual([]);
  });

  it("外部画像は最大20件まで（超過分は無視）", () => {
    const html = Array.from(
      { length: 25 },
      (_, i) => `<img src="https://ex.com/${i}.png">`,
    ).join("");
    expect(extractExternalImageUrls(html)).toHaveLength(20);
  });

  it("imgが無いHTMLでは空配列", () => {
    expect(extractExternalImageUrls("<p>本文</p>")).toEqual([]);
  });
});

describe("replaceExternalImageUrls", () => {
  it("取得済みURLをdata URIに置換する", () => {
    const out = replaceExternalImageUrls('<img src="https://ex.com/a.png" alt="a">', [
      { url: "https://ex.com/a.png", data_uri: "data:image/png;base64,AAAA" },
    ]);
    expect(out).toContain('src="data:image/png;base64,AAAA"');
    expect(out).toContain('alt="a"');
    expect(out).not.toContain("https://ex.com/a.png");
  });

  it("取得できなかったURLはそのまま残す（サニタイザが除去する）", () => {
    const out = replaceExternalImageUrls(
      '<img src="https://ex.com/a.png"><img src="https://ex.com/b.png">',
      [{ url: "https://ex.com/a.png", data_uri: "data:image/png;base64,AAAA" }],
    );
    expect(out).toContain("data:image/png;base64,AAAA");
    expect(out).toContain("https://ex.com/b.png");
  });

  it("data:image/以外のdata URIは差し込まない（多層防御）", () => {
    const out = replaceExternalImageUrls('<img src="https://ex.com/a.png">', [
      { url: "https://ex.com/a.png", data_uri: "data:text/html;base64,PGh0bWw+" },
    ]);
    expect(out).not.toContain("data:text/html");
    expect(out).toContain("https://ex.com/a.png");
  });
});
