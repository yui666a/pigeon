import { describe, it, expect } from "vitest";
import { resolveOpenableUrl } from "../utils/mailLinkPolicy";

describe("resolveOpenableUrl: メール本文リンクの許可判定", () => {
  it("http/https/mailtoはそのまま返す", () => {
    expect(resolveOpenableUrl("https://example.com/page")).toBe(
      "https://example.com/page",
    );
    expect(resolveOpenableUrl("http://example.com")).toBe("http://example.com");
    expect(resolveOpenableUrl("mailto:someone@example.com")).toBe(
      "mailto:someone@example.com",
    );
  });

  it("カスタムスキーム（deep-link含む）はnull", () => {
    expect(resolveOpenableUrl("com.haiso666.pigeon://oauth/callback?x=1")).toBeNull();
    expect(resolveOpenableUrl("javascript:alert(1)")).toBeNull();
    expect(resolveOpenableUrl("file:///etc/passwd")).toBeNull();
  });

  it("相対URL・空・nullはnull", () => {
    expect(resolveOpenableUrl("/local/path")).toBeNull();
    expect(resolveOpenableUrl("")).toBeNull();
    expect(resolveOpenableUrl(null)).toBeNull();
  });
});
