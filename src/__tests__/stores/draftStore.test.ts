import { describe, it, expect, vi, beforeEach } from "vitest";
import { useDraftStore } from "../../stores/draftStore";
import { useErrorStore } from "../../stores/errorStore";
import type { Draft } from "../../types/mail";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

function makeDraft(overrides: Partial<Draft> = {}): Draft {
  return {
    id: "d1",
    account_id: "acc1",
    to_addr: "tanaka@example.com",
    cc_addr: "",
    bcc_addr: "",
    subject: "件名",
    body_text: "本文",
    in_reply_to: null,
    created_at: "2026-07-12T10:00:00Z",
    updated_at: "2026-07-12T10:00:00Z",
    ...overrides,
  };
}

describe("draftStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useDraftStore.setState({ drafts: [], loading: false });
    useErrorStore.setState({ toasts: [] });
  });

  describe("fetchDrafts", () => {
    it("loads drafts for the account", async () => {
      mockInvoke.mockResolvedValue([makeDraft()]);
      await useDraftStore.getState().fetchDrafts("acc1");
      expect(mockInvoke).toHaveBeenCalledWith("get_drafts", {
        accountId: "acc1",
      });
      expect(useDraftStore.getState().drafts).toHaveLength(1);
    });

    it("reports an error via toast on failure", async () => {
      mockInvoke.mockRejectedValue("db error");
      await useDraftStore.getState().fetchDrafts("acc1");
      expect(useErrorStore.getState().toasts).toHaveLength(1);
      expect(useErrorStore.getState().toasts[0].kind).toBe("error");
    });
  });

  describe("saveDraft", () => {
    it("invokes save_draft and returns the saved draft", async () => {
      const saved = makeDraft({ id: "new-id" });
      mockInvoke.mockResolvedValue(saved);

      const req = {
        id: null,
        account_id: "acc1",
        to_addr: "a@ex.com",
        cc_addr: "",
        bcc_addr: "",
        subject: "S",
        body_text: "B",
        in_reply_to: null,
      };
      const result = await useDraftStore.getState().saveDraft(req);

      expect(mockInvoke).toHaveBeenCalledWith("save_draft", { req });
      expect(result).toEqual(saved);
    });

    it("does not throw when save fails, and returns null", async () => {
      mockInvoke.mockRejectedValue("smtp down");
      const req = {
        id: null,
        account_id: "acc1",
        to_addr: "a@ex.com",
        cc_addr: "",
        bcc_addr: "",
        subject: "",
        body_text: "",
        in_reply_to: null,
      };
      const result = await useDraftStore.getState().saveDraft(req);
      expect(result).toBeNull();
    });
  });

  describe("deleteDraft", () => {
    it("invokes delete_draft and removes it from state", async () => {
      mockInvoke.mockResolvedValue(undefined);
      useDraftStore.setState({ drafts: [makeDraft({ id: "d1" })] });

      await useDraftStore.getState().deleteDraft("d1");

      expect(mockInvoke).toHaveBeenCalledWith("delete_draft", { id: "d1" });
      expect(useDraftStore.getState().drafts).toHaveLength(0);
    });

    it("reports an error via toast on failure", async () => {
      mockInvoke.mockRejectedValue("db error");
      useDraftStore.setState({ drafts: [makeDraft({ id: "d1" })] });

      await useDraftStore.getState().deleteDraft("d1");

      expect(useErrorStore.getState().toasts).toHaveLength(1);
      // 失敗時は state から消さない
      expect(useDraftStore.getState().drafts).toHaveLength(1);
    });
  });
});
