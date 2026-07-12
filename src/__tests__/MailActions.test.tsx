import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import { MailActions } from "../components/mail-view/MailActions";
import { useComposeStore } from "../stores/composeStore";
import { useAccountStore } from "../stores/accountStore";
import type { Mail } from "../types/mail";

function makeMail(): Mail {
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
    fetched_at: "2026-07-10T10:00:00Z",
  };
}

describe("MailActions", () => {
  beforeEach(() => {
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
    useAccountStore.setState({ accounts: [], selectedAccountId: null });
  });

  it("renders reply, reply-all and forward buttons", () => {
    render(<MailActions mail={makeMail()} />);
    expect(screen.getByRole("button", { name: "返信" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "全員に返信" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "転送" })).toBeInTheDocument();
  });

  it("opens compose in reply mode with the mail", () => {
    render(<MailActions mail={makeMail()} />);
    fireEvent.click(screen.getByRole("button", { name: "返信" }));
    const s = useComposeStore.getState();
    expect(s.isOpen).toBe(true);
    expect(s.mode).toBe("reply");
    expect(s.to).toBe("tanaka@example.com");
    expect(s.replyToMailId).toBe("m1");
  });

  it("opens compose in replyAll mode", () => {
    render(<MailActions mail={makeMail()} />);
    fireEvent.click(screen.getByRole("button", { name: "全員に返信" }));
    expect(useComposeStore.getState().mode).toBe("replyAll");
  });

  it("opens compose in forward mode without replyToMailId", () => {
    render(<MailActions mail={makeMail()} />);
    fireEvent.click(screen.getByRole("button", { name: "転送" }));
    const s = useComposeStore.getState();
    expect(s.mode).toBe("forward");
    expect(s.replyToMailId).toBeNull();
    expect(s.subject).toBe("Fwd: 打ち合わせの件");
  });
});
