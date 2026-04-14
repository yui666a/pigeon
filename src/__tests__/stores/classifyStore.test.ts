import { describe, it, expect, vi, beforeEach } from "vitest";
import { useClassifyStore } from "../../stores/classifyStore";
import { useMailStore } from "../../stores/mailStore";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

describe("classifyStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useClassifyStore.setState({
      classifying: false,
      classifyingAccountId: null,
      progress: null,
      results: [],
      summary: null,
      error: null,
    });
    useMailStore.setState({
      unclassifiedMails: [],
      error: null,
    });
  });

  describe("classifyMail", () => {
    it("appends result on success", async () => {
      const result = { mail_id: "m1", action: "assign", confidence: 0.9, reason: "test" };
      mockInvoke.mockResolvedValue(result);

      await useClassifyStore.getState().classifyMail("m1");

      expect(useClassifyStore.getState().results).toHaveLength(1);
      expect(useClassifyStore.getState().results[0]).toEqual(result);
      expect(useClassifyStore.getState().classifying).toBe(false);
    });

    it("sets error on failure", async () => {
      mockInvoke.mockRejectedValue("classify error");

      await useClassifyStore.getState().classifyMail("m1");

      expect(useClassifyStore.getState().error).toBe("classify error");
      expect(useClassifyStore.getState().classifying).toBe(false);
    });
  });

  describe("approveClassification", () => {
    it("removes mail from mailStore.unclassifiedMails and classifyStore.results", async () => {
      useMailStore.setState({
        unclassifiedMails: [
          { id: "m1" } as never,
          { id: "m2" } as never,
        ],
      });
      useClassifyStore.setState({
        results: [
          { mail_id: "m1", action: "assign", confidence: 0.9, reason: "test" },
          { mail_id: "m2", action: "assign", confidence: 0.8, reason: "test" },
        ],
      });
      mockInvoke.mockResolvedValue(undefined);

      await useClassifyStore.getState().approveClassification("m1", "proj1");

      expect(useMailStore.getState().unclassifiedMails).toHaveLength(1);
      expect(useMailStore.getState().unclassifiedMails[0].id).toBe("m2");
      expect(useClassifyStore.getState().results).toHaveLength(1);
      expect(useClassifyStore.getState().results[0].mail_id).toBe("m2");
    });
  });

  describe("rejectClassification", () => {
    it("removes result but keeps mail in unclassified", async () => {
      useMailStore.setState({
        unclassifiedMails: [{ id: "m1" } as never],
      });
      useClassifyStore.setState({
        results: [{ mail_id: "m1", action: "assign", confidence: 0.5, reason: "test" }],
      });
      mockInvoke.mockResolvedValue(undefined);

      await useClassifyStore.getState().rejectClassification("m1");

      expect(useClassifyStore.getState().results).toHaveLength(0);
      expect(useMailStore.getState().unclassifiedMails).toHaveLength(1);
    });
  });

  describe("classifyAll", () => {
    it("sets classifying state with accountId", async () => {
      mockInvoke.mockResolvedValue(undefined);

      const promise = useClassifyStore.getState().classifyAll("acc1");

      expect(useClassifyStore.getState().classifyingAccountId).toBe("acc1");

      await promise;
    });

    it("clears state on error", async () => {
      mockInvoke.mockRejectedValue("ollama down");

      await useClassifyStore.getState().classifyAll("acc1");

      expect(useClassifyStore.getState().error).toBe("ollama down");
      expect(useClassifyStore.getState().classifying).toBe(false);
      expect(useClassifyStore.getState().classifyingAccountId).toBeNull();
    });
  });
});
