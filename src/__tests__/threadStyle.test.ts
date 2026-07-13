import { describe, it, expect } from "vitest";
import { threadBackgroundClass } from "../utils/threadStyle";
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
    subject: "テスト",
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

function makeThread(mails: Mail[]): Thread {
  return {
    thread_id: "<thread-1@example.com>",
    subject: "テスト",
    last_date: "2026-04-13T10:00:00+09:00",
    mail_count: mails.length,
    from_addrs: ["alice@example.com"],
    mails,
  };
}

describe("threadBackgroundClass", () => {
  it("returns grey when all mails are read", () => {
    const thread = makeThread([makeMail("m1"), makeMail("m2")]);
    expect(threadBackgroundClass(thread)).toBe("bg-gray-100");
  });

  it("returns empty (default) when the thread has unread mail", () => {
    const thread = makeThread([makeMail("m1"), makeMail("m2", { is_read: false })]);
    expect(threadBackgroundClass(thread)).toBe("");
  });

  it("returns blue when selected, regardless of read state", () => {
    const read = makeThread([makeMail("m1")]);
    const unread = makeThread([makeMail("m1", { is_read: false })]);
    expect(threadBackgroundClass(read, true)).toBe("bg-blue-50");
    expect(threadBackgroundClass(unread, true)).toBe("bg-blue-50");
  });

  it("defaults selected to false", () => {
    const thread = makeThread([makeMail("m1")]);
    expect(threadBackgroundClass(thread)).toBe("bg-gray-100");
  });
});
