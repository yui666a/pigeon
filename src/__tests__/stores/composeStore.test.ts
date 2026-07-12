import { describe, it, expect, vi, beforeEach } from "vitest";
import { useComposeStore } from "../../stores/composeStore";
import { useAccountStore } from "../../stores/accountStore";
import { useErrorStore } from "../../stores/errorStore";
import type { Account } from "../../types/account";
import type { Mail } from "../../types/mail";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

function makeAccount(): Account {
  return {
    id: "acc1",
    name: "Hiroshi",
    email: "me@example.com",
    imap_host: "imap.example.com",
    imap_port: 993,
    smtp_host: "smtp.example.com",
    smtp_port: 587,
    auth_type: "plain",
    provider: "other",
    needs_reauth: false,
    created_at: "2026-07-12T00:00:00Z",
  };
}

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
    body_text: "こんにちは。",
    body_html: null,
    date: "2026-07-10T10:00:00Z",
    has_attachments: false,
    raw_size: null,
    uid: 1,
    flags: null,
    is_read: false,
    fetched_at: "2026-07-10T10:00:00Z",
    ...overrides,
  };
}

describe("composeStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useComposeStore.setState({
      isOpen: false,
      mode: "new",
      to: "",
      cc: "",
      bcc: "",
      subject: "",
      body: "",
      sending: false,
      replyToMailId: null,
    });
    useAccountStore.setState({
      accounts: [makeAccount()],
      selectedAccountId: "acc1",
    });
    useErrorStore.setState({ errors: [] });
  });

  describe("openCompose", () => {
    it("opens empty for new mode", () => {
      useComposeStore.getState().openCompose("new");
      const s = useComposeStore.getState();
      expect(s.isOpen).toBe(true);
      expect(s.mode).toBe("new");
      expect(s.to).toBe("");
      expect(s.subject).toBe("");
      expect(s.replyToMailId).toBeNull();
    });

    it("prefills reply fields and keeps the source mail id", () => {
      useComposeStore.getState().openCompose("reply", makeMail());
      const s = useComposeStore.getState();
      expect(s.isOpen).toBe(true);
      expect(s.to).toBe("tanaka@example.com");
      expect(s.subject).toBe("Re: 打ち合わせの件");
      expect(s.body).toContain("> こんにちは。");
      expect(s.replyToMailId).toBe("m1");
    });

    it("excludes own address on replyAll using the selected account", () => {
      const mail = makeMail({ to_addr: "me@example.com, sato@example.com" });
      useComposeStore.getState().openCompose("replyAll", mail);
      expect(useComposeStore.getState().to).toBe(
        "tanaka@example.com, sato@example.com",
      );
    });

    it("does not set replyToMailId for forward", () => {
      useComposeStore.getState().openCompose("forward", makeMail());
      const s = useComposeStore.getState();
      expect(s.subject).toBe("Fwd: 打ち合わせの件");
      expect(s.replyToMailId).toBeNull();
    });
  });

  describe("send", () => {
    it("invokes send_mail with snake_case request and resets on success", async () => {
      mockInvoke.mockResolvedValue(undefined);
      useComposeStore.getState().openCompose("reply", makeMail());
      useComposeStore.setState({
        to: "tanaka@example.com, sato@example.com ,",
        cc: " suzuki@example.com ",
        bcc: "",
        body: "返信本文",
      });

      await useComposeStore.getState().send();

      expect(mockInvoke).toHaveBeenCalledWith("send_mail", {
        req: {
          account_id: "acc1",
          to: ["tanaka@example.com", "sato@example.com"],
          cc: ["suzuki@example.com"],
          bcc: [],
          subject: "Re: 打ち合わせの件",
          body_text: "返信本文",
          reply_to_mail_id: "m1",
        },
      });
      const s = useComposeStore.getState();
      expect(s.isOpen).toBe(false);
      expect(s.sending).toBe(false);
      expect(s.to).toBe("");
      expect(s.subject).toBe("");
      expect(s.body).toBe("");
      expect(s.replyToMailId).toBeNull();
    });

    it("sends reply_to_mail_id as null for new mail", async () => {
      mockInvoke.mockResolvedValue(undefined);
      useComposeStore.getState().openCompose("new");
      useComposeStore.setState({ to: "a@ex.com", subject: "S", body: "B" });

      await useComposeStore.getState().send();

      expect(mockInvoke).toHaveBeenCalledWith("send_mail", {
        req: expect.objectContaining({ reply_to_mail_id: null }),
      });
    });

    it("keeps the modal open and fields intact on failure, and reports the error", async () => {
      mockInvoke.mockRejectedValue("SMTP error: connection refused");
      useComposeStore.getState().openCompose("new");
      useComposeStore.setState({ to: "a@ex.com", subject: "S", body: "本文" });

      await useComposeStore.getState().send();

      const s = useComposeStore.getState();
      expect(s.isOpen).toBe(true);
      expect(s.sending).toBe(false);
      expect(s.to).toBe("a@ex.com");
      expect(s.subject).toBe("S");
      expect(s.body).toBe("本文");
      expect(useErrorStore.getState().errors).toHaveLength(1);
      expect(useErrorStore.getState().errors[0].message).toContain(
        "SMTP error",
      );
    });

    it("does nothing when no account is selected", async () => {
      useAccountStore.setState({ selectedAccountId: null });
      useComposeStore.getState().openCompose("new");
      useComposeStore.setState({ to: "a@ex.com" });

      await useComposeStore.getState().send();

      expect(mockInvoke).not.toHaveBeenCalled();
      expect(useComposeStore.getState().isOpen).toBe(true);
    });
  });

  describe("closeCompose", () => {
    it("closes and resets fields", () => {
      useComposeStore.getState().openCompose("reply", makeMail());
      useComposeStore.getState().closeCompose();
      const s = useComposeStore.getState();
      expect(s.isOpen).toBe(false);
      expect(s.to).toBe("");
      expect(s.body).toBe("");
    });
  });

  describe("setField", () => {
    it("updates a single field", () => {
      useComposeStore.getState().setField("to", "x@ex.com");
      useComposeStore.getState().setField("subject", "件名");
      expect(useComposeStore.getState().to).toBe("x@ex.com");
      expect(useComposeStore.getState().subject).toBe("件名");
    });
  });
});
