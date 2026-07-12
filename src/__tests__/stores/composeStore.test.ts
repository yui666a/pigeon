import { describe, it, expect, vi, beforeEach } from "vitest";
import { useComposeStore } from "../../stores/composeStore";
import { useAccountStore } from "../../stores/accountStore";
import { useErrorStore } from "../../stores/errorStore";
import { useDraftStore } from "../../stores/draftStore";
import type { Account } from "../../types/account";
import type { Draft, Mail } from "../../types/mail";

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
    is_flagged: false,
    fetched_at: "2026-07-10T10:00:00Z",
    ...overrides,
  };
}

describe("composeStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    useComposeStore.setState({
      isOpen: false,
      mode: "new",
      to: "",
      cc: "",
      bcc: "",
      subject: "",
      body: "",
      format: "plain",
      attachments: [],
      sending: false,
      replyToMailId: null,
      draftId: null,
    });
    useAccountStore.setState({
      accounts: [makeAccount()],
      selectedAccountId: "acc1",
    });
    useErrorStore.setState({ toasts: [] });
    useDraftStore.setState({ drafts: [], loading: false });
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
          body_html: null,
          attachments: [],
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

    it("shows a success toast on successful send", async () => {
      mockInvoke.mockResolvedValue(undefined);
      useComposeStore.getState().openCompose("new");
      useComposeStore.setState({ to: "a@ex.com", subject: "S", body: "B" });

      await useComposeStore.getState().send();

      const toasts = useErrorStore.getState().toasts;
      expect(toasts).toHaveLength(1);
      expect(toasts[0]).toMatchObject({
        kind: "success",
        message: "メールを送信しました",
      });
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
      const toasts = useErrorStore.getState().toasts;
      expect(toasts).toHaveLength(1);
      expect(toasts[0].kind).toBe("error");
      expect(toasts[0].message).toContain("SMTP error");
    });

    it("does nothing when no account is selected", async () => {
      useAccountStore.setState({ selectedAccountId: null });
      useComposeStore.getState().openCompose("new");
      useComposeStore.setState({ to: "a@ex.com" });

      await useComposeStore.getState().send();

      expect(mockInvoke).not.toHaveBeenCalled();
      expect(useComposeStore.getState().isOpen).toBe(true);
    });

    it("deletes the associated draft on successful send", async () => {
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === "send_mail") return Promise.resolve(undefined);
        if (cmd === "delete_draft") return Promise.resolve(undefined);
        return Promise.reject(new Error(`unexpected: ${cmd}`));
      });
      useComposeStore.getState().openCompose("new");
      useComposeStore.setState({
        to: "a@ex.com",
        subject: "S",
        body: "B",
        draftId: "draft-1",
      });

      await useComposeStore.getState().send();

      expect(mockInvoke).toHaveBeenCalledWith("delete_draft", {
        id: "draft-1",
      });
    });

    it("does not call delete_draft when there is no associated draft", async () => {
      mockInvoke.mockResolvedValue(undefined);
      useComposeStore.getState().openCompose("new");
      useComposeStore.setState({ to: "a@ex.com", subject: "S", body: "B" });

      await useComposeStore.getState().send();

      expect(mockInvoke).not.toHaveBeenCalledWith(
        "delete_draft",
        expect.anything(),
      );
    });
  });

  describe("openComposeFromDraft", () => {
    it("restores fields from a draft and tracks its id", () => {
      const draft: Draft = {
        id: "draft-1",
        account_id: "acc1",
        to_addr: "a@ex.com",
        cc_addr: "b@ex.com",
        bcc_addr: "",
        subject: "件名",
        body_text: "本文",
        in_reply_to: "m1",
        created_at: "2026-07-13T00:00:00Z",
        updated_at: "2026-07-13T00:00:00Z",
      };

      useComposeStore.getState().openComposeFromDraft(draft);

      const s = useComposeStore.getState();
      expect(s.isOpen).toBe(true);
      expect(s.to).toBe("a@ex.com");
      expect(s.cc).toBe("b@ex.com");
      expect(s.subject).toBe("件名");
      expect(s.body).toBe("本文");
      expect(s.replyToMailId).toBe("m1");
      expect(s.draftId).toBe("draft-1");
    });
  });

  describe("closeCompose", () => {
    it("closes and resets fields", async () => {
      mockInvoke.mockResolvedValue({
        id: "draft-1",
        account_id: "acc1",
        to_addr: "tanaka@example.com",
        cc_addr: "",
        bcc_addr: "",
        subject: "Re: 打ち合わせの件",
        body_text: "> こんにちは。",
        in_reply_to: "m1",
        created_at: "2026-07-13T00:00:00Z",
        updated_at: "2026-07-13T00:00:00Z",
      });
      useComposeStore.getState().openCompose("reply", makeMail());
      await useComposeStore.getState().closeCompose();
      const s = useComposeStore.getState();
      expect(s.isOpen).toBe(false);
      expect(s.to).toBe("");
      expect(s.body).toBe("");
    });

    it("auto-saves as a draft when there is input", async () => {
      const saved: Draft = {
        id: "draft-new",
        account_id: "acc1",
        to_addr: "a@ex.com",
        cc_addr: "",
        bcc_addr: "",
        subject: "S",
        body_text: "B",
        in_reply_to: null,
        created_at: "2026-07-13T00:00:00Z",
        updated_at: "2026-07-13T00:00:00Z",
      };
      mockInvoke.mockResolvedValue(saved);
      useComposeStore.getState().openCompose("new");
      useComposeStore.setState({ to: "a@ex.com", subject: "S", body: "B" });

      await useComposeStore.getState().closeCompose();

      expect(mockInvoke).toHaveBeenCalledWith("save_draft", {
        req: expect.objectContaining({
          id: null,
          account_id: "acc1",
          to_addr: "a@ex.com",
          subject: "S",
          body_text: "B",
        }),
      });
      expect(useComposeStore.getState().isOpen).toBe(false);
    });

    it("does not save a draft when all fields are empty", async () => {
      useComposeStore.getState().openCompose("new");

      await useComposeStore.getState().closeCompose();

      expect(mockInvoke).not.toHaveBeenCalledWith(
        "save_draft",
        expect.anything(),
      );
    });

    it("reuses the same draft id across repeated saves (upsert)", async () => {
      const firstSave: Draft = {
        id: "draft-1",
        account_id: "acc1",
        to_addr: "a@ex.com",
        cc_addr: "",
        bcc_addr: "",
        subject: "",
        body_text: "",
        in_reply_to: null,
        created_at: "2026-07-13T00:00:00Z",
        updated_at: "2026-07-13T00:00:00Z",
      };
      mockInvoke.mockResolvedValue(firstSave);
      useComposeStore.getState().openCompose("new");
      useComposeStore.setState({ to: "a@ex.com" });
      await useComposeStore.getState().closeCompose();

      useComposeStore.getState().openCompose("new");
      useComposeStore.setState({ to: "a@ex.com", draftId: "draft-1" });
      await useComposeStore.getState().closeCompose();

      expect(mockInvoke).toHaveBeenLastCalledWith("save_draft", {
        req: expect.objectContaining({ id: "draft-1" }),
      });
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

  describe("setFormat", () => {
    it("converts plain body to paragraph HTML when switching to rich", () => {
      useComposeStore.setState({ format: "plain", body: "1行目\n2行目" });
      useComposeStore.getState().setFormat("rich");
      const s = useComposeStore.getState();
      expect(s.format).toBe("rich");
      expect(s.body).toBe("<p>1行目</p><p>2行目</p>");
    });

    it("converts rich HTML back to plain text when switching to plain", () => {
      useComposeStore.setState({
        format: "rich",
        body: "<p>こんにちは</p><p>世界</p>",
      });
      useComposeStore.getState().setFormat("plain");
      const s = useComposeStore.getState();
      expect(s.format).toBe("plain");
      // マルチバイトが保持されること
      expect(s.body).toBe("こんにちは\n世界");
    });

    it("is a no-op when the format is unchanged", () => {
      useComposeStore.setState({ format: "plain", body: "そのまま" });
      useComposeStore.getState().setFormat("plain");
      expect(useComposeStore.getState().body).toBe("そのまま");
    });
  });

  describe("attachments", () => {
    it("adds attachments and de-duplicates by path", () => {
      useComposeStore.getState().addAttachments([
        { path: "/a.pdf", name: "a.pdf", size: 100 },
        { path: "/b.png", name: "b.png", size: 200 },
      ]);
      useComposeStore
        .getState()
        .addAttachments([{ path: "/a.pdf", name: "a.pdf", size: 100 }]);
      expect(useComposeStore.getState().attachments).toHaveLength(2);
    });

    it("removes an attachment by path", () => {
      useComposeStore.getState().addAttachments([
        { path: "/a.pdf", name: "a.pdf", size: 100 },
        { path: "/b.png", name: "b.png", size: 200 },
      ]);
      useComposeStore.getState().removeAttachment("/a.pdf");
      const s = useComposeStore.getState();
      expect(s.attachments).toHaveLength(1);
      expect(s.attachments[0].path).toBe("/b.png");
    });
  });

  describe("rich send", () => {
    it("sends body_html and empty body_text with attachment paths when rich", async () => {
      mockInvoke.mockResolvedValue(undefined);
      useComposeStore.getState().openCompose("new");
      useComposeStore.setState({
        to: "a@ex.com",
        subject: "S",
        format: "rich",
        body: "<p>本文</p>",
        attachments: [{ path: "/a.pdf", name: "a.pdf", size: 10 }],
      });

      await useComposeStore.getState().send();

      expect(mockInvoke).toHaveBeenCalledWith("send_mail", {
        req: expect.objectContaining({
          body_text: "",
          body_html: "<p>本文</p>",
          attachments: ["/a.pdf"],
        }),
      });
    });

    it("saves a rich draft as plain text (drafts table holds body_text only)", async () => {
      const saved: Draft = {
        id: "draft-r",
        account_id: "acc1",
        to_addr: "a@ex.com",
        cc_addr: "",
        bcc_addr: "",
        subject: "S",
        body_text: "こんにちは\n世界",
        in_reply_to: null,
        created_at: "2026-07-13T00:00:00Z",
        updated_at: "2026-07-13T00:00:00Z",
      };
      mockInvoke.mockResolvedValue(saved);
      useComposeStore.getState().openCompose("new");
      useComposeStore.setState({
        to: "a@ex.com",
        subject: "S",
        format: "rich",
        body: "<p>こんにちは</p><p>世界</p>",
      });

      await useComposeStore.getState().closeCompose();

      expect(mockInvoke).toHaveBeenCalledWith("save_draft", {
        req: expect.objectContaining({ body_text: "こんにちは\n世界" }),
      });
    });
  });
});
