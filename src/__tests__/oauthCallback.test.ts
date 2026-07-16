import { describe, it, expect } from "vitest";
import { isOAuthCallbackUrl } from "../utils/oauthCallback";

describe("isOAuthCallbackUrl", () => {
  it("自アプリスキームの oauth/callback を受理する", () => {
    expect(
      isOAuthCallbackUrl("com.haiso666.pigeon://oauth/callback?code=abc&state=xyz"),
    ).toBe(true);
    expect(isOAuthCallbackUrl("com.haiso666.pigeon://oauth/callback")).toBe(true);
  });

  it("loopback(127.0.0.1)の /oauth/callback を受理する（バックエンドの実フロー）", () => {
    // start_loopback_callback_listener は http://127.0.0.1:<任意ポート>/oauth/callback
    // にリダイレクトを受ける。ポートは動的なので任意ポートを許容する
    expect(
      isOAuthCallbackUrl("http://127.0.0.1:52345/oauth/callback?code=abc&state=xyz"),
    ).toBe(true);
    expect(isOAuthCallbackUrl("http://127.0.0.1:1/oauth/callback")).toBe(true);
  });

  it("loopback以外のホストの http コールバックは拒否する（SSRF/偽装対策）", () => {
    // 外部ホストやlocalhost名前解決経由の偽装を防ぐ。許可は 127.0.0.1 リテラルのみ
    expect(isOAuthCallbackUrl("http://evil.example:52345/oauth/callback?code=x")).toBe(false);
    expect(isOAuthCallbackUrl("http://localhost:52345/oauth/callback?code=x")).toBe(false);
    expect(isOAuthCallbackUrl("http://127.0.0.1.evil.com/oauth/callback")).toBe(false);
    // https でも 127.0.0.1 以外は不可
    expect(isOAuthCallbackUrl("https://10.0.0.1/oauth/callback")).toBe(false);
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
