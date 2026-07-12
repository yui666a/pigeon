import { describe, it, expect, beforeEach } from "vitest";
import { useSelectionStore } from "../../stores/selectionStore";
import type { Mail, Thread } from "../../types/mail";

function makeMail(id: string): Mail {
  return {
    id,
    account_id: "acc1",
    folder: "INBOX",
    message_id: `<${id}@example.com>`,
    in_reply_to: null,
    references: null,
    from_addr: "sender@example.com",
    to_addr: "me@example.com",
    cc_addr: null,
    subject: "Subject",
    body_text: "body",
    body_html: null,
    date: "2026-07-13T10:00:00",
    has_attachments: false,
    raw_size: null,
    uid: 1,
    flags: null,
    is_read: false,
    is_flagged: false,
    fetched_at: "2026-07-13T00:00:00",
  };
}

function makeThread(threadId: string, mailIds: string[]): Thread {
  const mails = mailIds.map(makeMail);
  return {
    thread_id: threadId,
    subject: "Subject",
    last_date: mails[mails.length - 1].date,
    mail_count: mails.length,
    from_addrs: [],
    mails,
  };
}

describe("selectionStore", () => {
  beforeEach(() => {
    useSelectionStore.setState({ selectedThreadIds: new Set() });
  });

  describe("toggleThread", () => {
    it("selects an unselected thread", () => {
      const t1 = makeThread("t1", ["m1", "m2"]);
      useSelectionStore.getState().toggleThread(t1);
      expect(useSelectionStore.getState().isSelected("t1")).toBe(true);
    });

    it("deselects an already selected thread", () => {
      const t1 = makeThread("t1", ["m1"]);
      useSelectionStore.getState().toggleThread(t1);
      useSelectionStore.getState().toggleThread(t1);
      expect(useSelectionStore.getState().isSelected("t1")).toBe(false);
    });
  });

  describe("clear", () => {
    it("empties the selection", () => {
      useSelectionStore.getState().toggleThread(makeThread("t1", ["m1"]));
      useSelectionStore.getState().toggleThread(makeThread("t2", ["m2"]));
      useSelectionStore.getState().clear();
      expect(useSelectionStore.getState().selectedThreadIds.size).toBe(0);
    });
  });

  describe("selectedMailIds", () => {
    it("flattens mail ids from selected threads only", () => {
      const t1 = makeThread("t1", ["m1", "m2"]);
      const t2 = makeThread("t2", ["m3"]);
      useSelectionStore.getState().toggleThread(t1);

      const ids = useSelectionStore.getState().selectedMailIds([t1, t2]);
      expect(ids).toEqual(["m1", "m2"]);
    });

    it("returns empty array when nothing selected", () => {
      const t1 = makeThread("t1", ["m1"]);
      const ids = useSelectionStore.getState().selectedMailIds([t1]);
      expect(ids).toEqual([]);
    });

    it("ignores selected thread ids no longer present in the given threads", () => {
      const t1 = makeThread("t1", ["m1"]);
      useSelectionStore.getState().toggleThread(t1);
      // t1 はもう一覧に無い（再読み込みで消えた）想定
      const ids = useSelectionStore.getState().selectedMailIds([]);
      expect(ids).toEqual([]);
    });
  });
});
