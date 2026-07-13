import { describe, it, expect } from "vitest";
import {
  ApiError,
  toApiError,
  isReauthError,
  errorMessage,
} from "../../api/errors";

describe("toApiError", () => {
  it("文字列の失敗をそのままのメッセージで ApiError に包む", () => {
    const err = toApiError("IMAP error: connection failed");
    expect(err).toBeInstanceOf(ApiError);
    expect(err.message).toBe("IMAP error: connection failed");
    expect(err.kind).toBe("unknown");
  });

  it("'Reauth required' を含む失敗は kind 'reauth' に分類する", () => {
    const err = toApiError("Reauth required: acc1");
    expect(err.kind).toBe("reauth");
    expect(err.message).toBe("Reauth required: acc1");
  });

  it("Error インスタンスは message を使う（'Error:' プレフィックスを付けない）", () => {
    const err = toApiError(new Error("network error"));
    expect(err.message).toBe("network error");
    expect(err.kind).toBe("unknown");
  });

  it("ApiError はそのまま返す（二重ラップしない）", () => {
    const original = new ApiError("reauth", "Reauth required: acc1");
    expect(toApiError(original)).toBe(original);
  });
});

describe("isReauthError", () => {
  it("kind が reauth の ApiError で true", () => {
    expect(isReauthError(new ApiError("reauth", "Reauth required: acc1"))).toBe(
      true,
    );
  });

  it("kind が unknown の ApiError で false", () => {
    expect(isReauthError(new ApiError("unknown", "boom"))).toBe(false);
  });

  it("ApiError 以外（生文字列など）は正規化してから判定する", () => {
    expect(isReauthError("Reauth required: acc1")).toBe(true);
    expect(isReauthError("IMAP down")).toBe(false);
  });
});

describe("errorMessage", () => {
  it("ApiError の message を返す", () => {
    expect(errorMessage(new ApiError("unknown", "db error"))).toBe("db error");
  });

  it("文字列はそのまま返す（従来の String(e) と同じ表示になる）", () => {
    expect(errorMessage("smtp down")).toBe("smtp down");
  });
});
