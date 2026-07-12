import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { MailBody } from "../components/mail-view/MailBody";
import type { Mail } from "../types/mail";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(),
}));

function makeMail(overrides: Partial<Mail> = {}): Mail {
  return {
    id: "m1", account_id: "acc1", folder: "INBOX",
    message_id: "<msg1@example.com>", in_reply_to: null, references: null,
    from_addr: "alice@example.com", to_addr: "bob@example.com",
    cc_addr: null, subject: "件名",
    body_text: "本文テキスト", body_html: null,
    date: "2026-07-12T10:00:00+09:00", has_attachments: false,
    raw_size: null, uid: 1, flags: null, is_read: false, fetched_at: "2026-07-12T00:00:00",
    ...overrides,
  };
}

describe("MailBody", () => {
  it("renders body text", () => {
    render(<MailBody mail={makeMail()} />);
    expect(screen.getByText("本文テキスト")).toBeInTheDocument();
  });

  it("has_attachments のとき添付セクションを表示する", () => {
    render(<MailBody mail={makeMail({ has_attachments: true })} />);
    expect(
      screen.getByRole("button", { name: /添付ファイルを表示/ }),
    ).toBeInTheDocument();
  });

  it("添付がないメールでは添付セクションを表示しない", () => {
    render(<MailBody mail={makeMail({ has_attachments: false })} />);
    expect(
      screen.queryByRole("button", { name: /添付ファイルを表示/ }),
    ).not.toBeInTheDocument();
  });
});
