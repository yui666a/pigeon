import { describe, it, expect } from "vitest";
import { sanitizeMailHtml } from "../utils/sanitizeMailHtml";

describe("sanitizeMailHtml: 外部画像の除去", () => {
  it("外部http(s)画像のsrcを除去する（トラッキングピクセル対策）", () => {
    // CSP img-src が第一防御だが、サニタイズ側でも除去して単独依存にしない
    const out = sanitizeMailHtml(
      '<img src="https://tracker.example/pixel.gif" alt="logo"><img src="http://ex.com/a.png">',
    );
    expect(out).not.toContain("tracker.example");
    expect(out).not.toContain("http://ex.com");
    expect(out).toContain('alt="logo"');
  });

  it("プロトコル相対URLの画像も除去する", () => {
    const out = sanitizeMailHtml('<img src="//tracker.example/p.gif">');
    expect(out).not.toContain("tracker.example");
  });

  it("インライン画像（data:image）は保持する", () => {
    const out = sanitizeMailHtml('<img src="data:image/png;base64,AAAA">');
    expect(out).toContain("data:image/png;base64,AAAA");
  });

  it("未解決のcid:参照は保持する（外部リクエストは発生しない）", () => {
    const out = sanitizeMailHtml('<img src="cid:logo@ex.com">');
    expect(out).toContain("cid:logo@ex.com");
  });

  it("srcsetは常に除去する（外部URLの迂回経路）", () => {
    const out = sanitizeMailHtml(
      '<img src="data:image/png;base64,AAAA" srcset="https://tracker.example/p.gif 1x">',
    );
    expect(out).not.toContain("srcset");
    expect(out).not.toContain("tracker.example");
  });
});
