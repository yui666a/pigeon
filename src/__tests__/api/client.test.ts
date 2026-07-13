import { describe, it, expect, vi, beforeEach } from "vitest";
import { invokeCommand } from "../../api/client";
import { ApiError } from "../../api/errors";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

describe("invokeCommand", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("コマンド名と引数を invoke に渡し、結果をそのまま返す", async () => {
    mockInvoke.mockResolvedValue(42);

    const result = await invokeCommand<number>("sync_account", {
      accountId: "acc1",
    });

    expect(mockInvoke).toHaveBeenCalledWith("sync_account", {
      accountId: "acc1",
    });
    expect(result).toBe(42);
  });

  it("引数省略時は undefined を渡す", async () => {
    mockInvoke.mockResolvedValue([]);

    await invokeCommand("get_accounts");

    expect(mockInvoke).toHaveBeenCalledWith("get_accounts", undefined);
  });

  it("失敗時は ApiError に正規化して投げる（メッセージは元の文字列のまま）", async () => {
    mockInvoke.mockRejectedValue("IMAP error: EXPUNGE failed");

    await expect(invokeCommand("delete_mail", { mailId: "m1" })).rejects.toThrow(
      ApiError,
    );
    await expect(
      invokeCommand("delete_mail", { mailId: "m1" }),
    ).rejects.toMatchObject({
      kind: "unknown",
      message: "IMAP error: EXPUNGE failed",
    });
  });

  it("Reauth required を含む失敗は kind 'reauth' の ApiError になる", async () => {
    mockInvoke.mockRejectedValue("Reauth required: acc1");

    await expect(
      invokeCommand("sync_account", { accountId: "acc1" }),
    ).rejects.toMatchObject({
      kind: "reauth",
      message: "Reauth required: acc1",
    });
  });
});
