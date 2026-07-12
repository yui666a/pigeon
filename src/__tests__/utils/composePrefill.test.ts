import { describe, it, expect } from "vitest";
import { buildPrefill, splitRecipients } from "../../utils/composePrefill";
import type { Mail } from "../../types/mail";

function makeMail(overrides: Partial<Mail> = {}): Mail {
  return {
    id: "m1",
    account_id: "acc1",
    folder: "INBOX",
    message_id: "<orig@ex.com>",
    in_reply_to: null,
    references: null,
    from_addr: "tanaka@example.com",
    to_addr: "me@example.com",
    cc_addr: null,
    subject: "打ち合わせの件",
    body_text: "こんにちは。\nよろしくお願いします。",
    body_html: null,
    date: "2026-07-10T10:00:00Z",
    has_attachments: false,
    raw_size: null,
    uid: 1,
    flags: null,
    fetched_at: "2026-07-10T10:00:00Z",
    ...overrides,
  };
}

describe("buildPrefill", () => {
  describe("new", () => {
    it("returns empty fields", () => {
      const p = buildPrefill("new", null, "me@example.com");
      expect(p).toEqual({ to: "", cc: "", bcc: "", subject: "", body: "" });
    });
  });

  describe("reply", () => {
    it("sets To to the original sender and prefixes subject with Re:", () => {
      const p = buildPrefill("reply", makeMail(), "me@example.com");
      expect(p.to).toBe("tanaka@example.com");
      expect(p.cc).toBe("");
      expect(p.subject).toBe("Re: 打ち合わせの件");
    });

    it("does not duplicate Re: prefix (case-insensitive)", () => {
      const p1 = buildPrefill(
        "reply",
        makeMail({ subject: "Re: 打ち合わせの件" }),
        "me@example.com",
      );
      expect(p1.subject).toBe("Re: 打ち合わせの件");

      const p2 = buildPrefill(
        "reply",
        makeMail({ subject: "RE: 打ち合わせの件" }),
        "me@example.com",
      );
      expect(p2.subject).toBe("RE: 打ち合わせの件");
    });

    it("quotes the original body with a header and > prefix per line", () => {
      const p = buildPrefill("reply", makeMail(), "me@example.com");
      expect(p.body).toContain("tanaka@example.com:");
      expect(p.body).toContain("> こんにちは。");
      expect(p.body).toContain("> よろしくお願いします。");
    });

    it("quotes empty body when body_text is null", () => {
      const p = buildPrefill(
        "reply",
        makeMail({ body_text: null }),
        "me@example.com",
      );
      expect(p.body).toContain("tanaka@example.com:");
      expect(p.body).toContain("> ");
    });
  });

  describe("replyAll", () => {
    it("puts original From and To into To, original Cc into Cc, excluding self", () => {
      const mail = makeMail({
        to_addr: "me@example.com, sato@example.com",
        cc_addr: "suzuki@example.com, ME@EXAMPLE.COM",
      });
      const p = buildPrefill("replyAll", mail, "me@example.com");
      expect(p.to).toBe("tanaka@example.com, sato@example.com");
      expect(p.cc).toBe("suzuki@example.com");
    });

    it("excludes self case-insensitively from To", () => {
      const mail = makeMail({ to_addr: "Me@Example.Com" });
      const p = buildPrefill("replyAll", mail, "me@example.com");
      expect(p.to).toBe("tanaka@example.com");
    });

    it("prefixes subject with Re: like reply", () => {
      const p = buildPrefill("replyAll", makeMail(), "me@example.com");
      expect(p.subject).toBe("Re: 打ち合わせの件");
    });
  });

  describe("forward", () => {
    it("leaves To empty and prefixes subject with Fwd:", () => {
      const p = buildPrefill("forward", makeMail(), "me@example.com");
      expect(p.to).toBe("");
      expect(p.subject).toBe("Fwd: 打ち合わせの件");
    });

    it("does not duplicate Fwd: prefix (case-insensitive)", () => {
      const p = buildPrefill(
        "forward",
        makeMail({ subject: "FWD: 打ち合わせの件" }),
        "me@example.com",
      );
      expect(p.subject).toBe("FWD: 打ち合わせの件");
    });

    it("quotes the original body", () => {
      const p = buildPrefill("forward", makeMail(), "me@example.com");
      expect(p.body).toContain("tanaka@example.com:");
      expect(p.body).toContain("> こんにちは。");
    });
  });
});

describe("splitRecipients", () => {
  it("splits a comma-separated string, trimming whitespace", () => {
    expect(splitRecipients("a@ex.com, b@ex.com ,c@ex.com")).toEqual([
      "a@ex.com",
      "b@ex.com",
      "c@ex.com",
    ]);
  });

  it("removes empty entries", () => {
    expect(splitRecipients("a@ex.com,, ,")).toEqual(["a@ex.com"]);
    expect(splitRecipients("")).toEqual([]);
  });
});
