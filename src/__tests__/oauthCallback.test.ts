import { describe, it, expect } from "vitest";
import { isOAuthCallbackUrl } from "../utils/oauthCallback";

describe("isOAuthCallbackUrl", () => {
  it("自アプリスキームの oauth/callback を受理する", () => {
    expect(
      isOAuthCallbackUrl("com.haiso666.pigeon://oauth/callback?code=abc&state=xyz"),
    ).toBe(true);
    expect(isOAuthCallbackUrl("com.haiso666.pigeon://oauth/callback")).toBe(true);
  });

  it("部分文字列に oauth/callback を含むだけの偽装URLを拒否する", () => {
    expect(isOAuthCallbackUrl("https://evil.example/oauth/callback?code=x")).toBe(false);
    expect(
      isOAuthCallbackUrl("com.haiso666.pigeon://evil/oauth/callback?code=x"),
    ).toBe(false);
    expect(
      isOAuthCallbackUrl("evil.scheme://oauth/callback?code=x"),
    ).toBe(false);
  });

  it("パスが異なるURLを拒否する", () => {
    expect(isOAuthCallbackUrl("com.haiso666.pigeon://oauth/callback/extra")).toBe(false);
    expect(isOAuthCallbackUrl("com.haiso666.pigeon://other/path")).toBe(false);
  });

  it("URLとして不正な文字列を拒否する", () => {
    expect(isOAuthCallbackUrl("not a url")).toBe(false);
    expect(isOAuthCallbackUrl("")).toBe(false);
  });
});
