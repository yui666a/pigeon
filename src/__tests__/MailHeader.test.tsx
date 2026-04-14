import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { MailHeader } from "../components/mail-view/MailHeader";
import type { Mail } from "../types/mail";

function makeMail(overrides: Partial<Mail> = {}): Mail {
  return {
    id: "m1", account_id: "acc1", folder: "INBOX",
    message_id: "<msg1@example.com>", in_reply_to: null, references: null,
    from_addr: "Alice <alice@example.com>", to_addr: "bob@example.com",
    cc_addr: null, subject: "テストメール件名",
    body_text: "本文", body_html: null,
    date: "2026-04-13T10:00:00+09:00", has_attachments: false,
    raw_size: null, uid: 1, flags: null, fetched_at: "2026-04-13T00:00:00",
    ...overrides,
  };
}

describe("MailHeader", () => {
  it("renders subject", () => {
    render(<MailHeader mail={makeMail()} />);
    expect(screen.getByText("テストメール件名")).toBeInTheDocument();
  });

  it("renders from address", () => {
    render(<MailHeader mail={makeMail()} />);
    expect(screen.getByText("Alice <alice@example.com>")).toBeInTheDocument();
  });

  it("renders to address", () => {
    render(<MailHeader mail={makeMail()} />);
    expect(screen.getByText("bob@example.com")).toBeInTheDocument();
  });

  it("renders cc when present", () => {
    render(<MailHeader mail={makeMail({ cc_addr: "cc@example.com" })} />);
    expect(screen.getByText("cc@example.com")).toBeInTheDocument();
    expect(screen.getByText("Cc:")).toBeInTheDocument();
  });

  it("hides cc when null", () => {
    render(<MailHeader mail={makeMail({ cc_addr: null })} />);
    expect(screen.queryByText("Cc:")).not.toBeInTheDocument();
  });

  it("renders formatted date", () => {
    render(<MailHeader mail={makeMail()} />);
    expect(screen.getByText("Date:")).toBeInTheDocument();
  });
});
