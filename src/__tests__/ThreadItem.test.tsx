import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ThreadItem } from "../components/thread-list/ThreadItem";
import type { Mail, Thread } from "../types/mail";

function makeMail(id: string, overrides: Partial<Mail> = {}): Mail {
  return {
    id,
    account_id: "acc1",
    folder: "INBOX",
    message_id: `<${id}@example.com>`,
    in_reply_to: null,
    references: null,
    from_addr: "alice@example.com",
    to_addr: "me@example.com",
    cc_addr: null,
    subject: "テストスレッド",
    body_text: "body",
    body_html: null,
    date: "2026-04-13T10:00:00+09:00",
    has_attachments: false,
    raw_size: null,
    uid: 1,
    flags: null,
    is_read: true,
    is_flagged: false,
    fetched_at: "2026-04-13T00:00:00",
    ...overrides,
  };
}

function makeThread(overrides: Partial<Thread> = {}): Thread {
  return {
    thread_id: "<thread-1@example.com>",
    subject: "テストスレッド",
    last_date: "2026-04-13T10:00:00+09:00",
    mail_count: 1,
    from_addrs: ["alice@example.com"],
    mails: [],
    projects: [],
    ...overrides,
  };
}

describe("ThreadItem", () => {
  it("renders subject and date", () => {
    render(<ThreadItem thread={makeThread()} selected={false} onClick={vi.fn()} />);
    expect(screen.getByText("テストスレッド")).toBeInTheDocument();
    expect(screen.getByText("4/13")).toBeInTheDocument();
  });

  it("renders from addresses", () => {
    render(<ThreadItem thread={makeThread({ from_addrs: ["alice@example.com", "bob@example.com"] })} selected={false} onClick={vi.fn()} />);
    expect(screen.getByText("alice@example.com, bob@example.com")).toBeInTheDocument();
  });

  it("shows mail count badge when > 1", () => {
    render(<ThreadItem thread={makeThread({ mail_count: 5 })} selected={false} onClick={vi.fn()} />);
    expect(screen.getByText("5")).toBeInTheDocument();
  });

  it("hides mail count badge for single mail", () => {
    render(<ThreadItem thread={makeThread({ mail_count: 1 })} selected={false} onClick={vi.fn()} />);
    expect(screen.queryByText("1")).not.toBeInTheDocument();
  });

  it("applies selected style", () => {
    const { container } = render(<ThreadItem thread={makeThread()} selected={true} onClick={vi.fn()} />);
    const item = container.firstElementChild!;
    expect(item.className).toContain("bg-blue-50");
  });

  it("renders subject in bold when thread has unread mail", () => {
    const thread = makeThread({
      mails: [makeMail("m1"), makeMail("m2", { is_read: false })],
    });
    render(<ThreadItem thread={thread} selected={false} onClick={vi.fn()} />);
    expect(screen.getByText("テストスレッド").className).toContain("font-bold");
  });

  it("renders subject with normal weight when all mails are read", () => {
    const thread = makeThread({ mails: [makeMail("m1"), makeMail("m2")] });
    render(<ThreadItem thread={thread} selected={false} onClick={vi.fn()} />);
    const subject = screen.getByText("テストスレッド");
    expect(subject.className).not.toContain("font-bold");
    expect(subject.className).toContain("font-medium");
  });

  it("applies a grey background when all mails are read", () => {
    const thread = makeThread({ mails: [makeMail("m1"), makeMail("m2")] });
    const { container } = render(
      <ThreadItem thread={thread} selected={false} onClick={vi.fn()} />,
    );
    expect(container.firstElementChild!.className).toContain("bg-gray-100");
  });

  it("does not apply the grey background when the thread has unread mail", () => {
    const thread = makeThread({
      mails: [makeMail("m1"), makeMail("m2", { is_read: false })],
    });
    const { container } = render(
      <ThreadItem thread={thread} selected={false} onClick={vi.fn()} />,
    );
    expect(container.firstElementChild!.className).not.toContain("bg-gray-100");
  });

  it("prioritizes the selected background over the read-state background", () => {
    const thread = makeThread({ mails: [makeMail("m1"), makeMail("m2")] });
    const { container } = render(
      <ThreadItem thread={thread} selected={true} onClick={vi.fn()} />,
    );
    expect(container.firstElementChild!.className).toContain("bg-blue-50");
    expect(container.firstElementChild!.className).not.toContain("bg-gray-100");
  });

  it("shows a star when the thread contains a flagged mail", () => {
    const thread = makeThread({
      mails: [makeMail("m1"), makeMail("m2", { is_flagged: true })],
    });
    render(<ThreadItem thread={thread} selected={false} onClick={vi.fn()} />);
    expect(screen.getByText("★")).toBeInTheDocument();
  });

  it("hides the star when no mail in the thread is flagged", () => {
    const thread = makeThread({ mails: [makeMail("m1"), makeMail("m2")] });
    render(<ThreadItem thread={thread} selected={false} onClick={vi.fn()} />);
    expect(screen.queryByText("★")).not.toBeInTheDocument();
  });

  it("calls onClick when clicked", () => {
    const onClick = vi.fn();
    render(<ThreadItem thread={makeThread()} selected={false} onClick={onClick} />);
    // mousedown + mouseup without move triggers onClick
    const item = screen.getByText("テストスレッド").closest("div[class]")!;
    fireEvent.mouseDown(item);
    fireEvent.mouseUp(window);
    expect(onClick).toHaveBeenCalledTimes(1);
  });
});
