import { describe, it, expect, vi, beforeEach } from "vitest";
import { mailApi } from "../../api/mailApi";
import { ApiError } from "../../api/errors";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

describe("mailApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("fetchThreads は get_threads を accountId/folder 付きで呼ぶ", async () => {
    mockInvoke.mockResolvedValue([]);

    const threads = await mailApi.fetchThreads("acc1", "INBOX");

    expect(mockInvoke).toHaveBeenCalledWith("get_threads", {
      accountId: "acc1",
      folder: "INBOX",
    });
    expect(threads).toEqual([]);
  });

  it("syncAccount は sync_account を呼び、取り込み件数を返す", async () => {
    mockInvoke.mockResolvedValue(7);

    const count = await mailApi.syncAccount("acc1");

    expect(mockInvoke).toHaveBeenCalledWith("sync_account", {
      accountId: "acc1",
    });
    expect(count).toBe(7);
  });

  it("bulkMoveMails は bulk_move_mails に mailIds/projectId を渡す", async () => {
    const result = { succeeded: ["m1"], failed: [] };
    mockInvoke.mockResolvedValue(result);

    const res = await mailApi.bulkMoveMails(["m1"], "p1");

    expect(mockInvoke).toHaveBeenCalledWith("bulk_move_mails", {
      mailIds: ["m1"],
      projectId: "p1",
    });
    expect(res).toEqual(result);
  });

  it("失敗は ApiError として伝播する", async () => {
    mockInvoke.mockRejectedValue("Reauth required: acc1");

    await expect(mailApi.syncAccount("acc1")).rejects.toMatchObject({
      kind: "reauth",
      message: "Reauth required: acc1",
    });
    await expect(mailApi.syncAccount("acc1")).rejects.toBeInstanceOf(ApiError);
  });
});
