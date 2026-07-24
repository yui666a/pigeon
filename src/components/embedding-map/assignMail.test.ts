import { describe, it, expect, vi } from "vitest";
import { assignAndNotify } from "./assignMail";
import type { BulkResult } from "../../types/mail";
import type { MapProject } from "../../types/embeddingMap";

const project: MapProject = { id: "p1", name: "案件A", color: null };

const okResult: BulkResult = { succeeded: ["m1"], failed: [] };
const ngResult: BulkResult = { succeeded: [], failed: [["m1", "boom"]] };

describe("assignAndNotify", () => {
  it("成功したら mail-assigned を emit して assigned を返す", async () => {
    const emit = vi.fn().mockResolvedValue(undefined);
    const bulkMove = vi.fn().mockResolvedValue(okResult);
    const outcome = await assignAndNotify("m1", project, { bulkMove, emit });
    expect(outcome).toBe("assigned");
    expect(bulkMove).toHaveBeenCalledWith(["m1"], "p1");
    expect(emit).toHaveBeenCalledWith("mail-assigned", {
      mail_id: "m1",
      project_id: "p1",
    });
  });

  it("command が失敗を返したら emit せず failed を返す", async () => {
    const emit = vi.fn();
    const bulkMove = vi.fn().mockResolvedValue(ngResult);
    const outcome = await assignAndNotify("m1", project, { bulkMove, emit });
    expect(outcome).toBe("failed");
    expect(emit).not.toHaveBeenCalled();
  });

  it("invoke が例外を投げたら failed を返す", async () => {
    const emit = vi.fn();
    const bulkMove = vi.fn().mockRejectedValue(new Error("ipc error"));
    const outcome = await assignAndNotify("m1", project, { bulkMove, emit });
    expect(outcome).toBe("failed");
    expect(emit).not.toHaveBeenCalled();
  });
});
