import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ThreadDragItem } from "../components/thread-list/ThreadDragItem";
import type { Mail, Thread } from "../types/mail";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve([])),
}));

const mail = (id: string, isRead: boolean): Mail => ({
  id,
  account_id: "acc1",
  folder: "INBOX",
  message_id: `<${id}@example.com>`,
  in_reply_to: null,
  references: null,
  from_addr: `${id}@example.com`,
  to_addr: "me@example.com",
  cc_addr: null,
  subject: "Re: Test",
  body_text: "本文",
  body_html: null,
  date: "2026-07-12T00:00:00Z",
  has_attachments: false,
  raw_size: null,
  uid: 1,
  flags: null,
  is_read: isRead,
  fetched_at: "2026-07-12T00:00:00Z",
});

const thread = (mails: Mail[]): Thread => ({
  thread_id: mails[0].message_id,
  subject: mails[0].subject,
  last_date: mails[mails.length - 1].date,
  mail_count: mails.length,
  from_addrs: mails.map((m) => m.from_addr),
  mails,
});

describe("ThreadDragItem", () => {
  it("複数メールのスレッドは件数バッジを表示する", () => {
    render(
      <ThreadDragItem
        thread={thread([mail("m1", true), mail("m2", true)])}
        onClick={() => {}}
      />,
    );
    expect(screen.getByText("(2)")).toBeInTheDocument();
    // 最新メールの送信者が表示される
    expect(screen.getByText("m2@example.com")).toBeInTheDocument();
  });

  it("1通のスレッドは件数バッジを出さない", () => {
    render(<ThreadDragItem thread={thread([mail("m1", true)])} onClick={() => {}} />);
    expect(screen.queryByText("(1)")).not.toBeInTheDocument();
  });

  it("未読メールを含むスレッドは件名が太字になる", () => {
    render(
      <ThreadDragItem
        thread={thread([mail("m1", true), mail("m2", false)])}
        onClick={() => {}}
      />,
    );
    const subject = screen.getByText("Re: Test");
    expect(subject.className).toContain("font-bold");
  });
});
