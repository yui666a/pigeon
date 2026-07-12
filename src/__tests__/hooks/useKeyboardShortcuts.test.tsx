import { render, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import { useKeyboardShortcuts } from "../../hooks/useKeyboardShortcuts";
import { useComposeStore } from "../../stores/composeStore";
import { useMailStore } from "../../stores/mailStore";
import { useAccountStore } from "../../stores/accountStore";
import type { Mail, Thread } from "../../types/mail";

function makeMail(id = "m1"): Mail {
  return {
    id,
    account_id: "acc1",
    folder: "INBOX",
    message_id: `<${id}@ex.com>`,
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

function ShortcutHost() {
  useKeyboardShortcuts();
  return (
    <div>
      <input data-testid="text-input" />
      <textarea data-testid="text-area" />
      <div data-testid="editable" contentEditable />
    </div>
  );
}

describe("useKeyboardShortcuts", () => {
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
    useMailStore.setState({ selectedMail: null, selectedThread: null });
    useAccountStore.setState({ accounts: [], selectedAccountId: null });
  });

  it("opens new compose on n", () => {
    render(<ShortcutHost />);
    fireEvent.keyDown(document.body, { key: "n" });
    const s = useComposeStore.getState();
    expect(s.isOpen).toBe(true);
    expect(s.mode).toBe("new");
  });

  it("does not fire while an input is focused", () => {
    const { getByTestId } = render(<ShortcutHost />);
    fireEvent.keyDown(getByTestId("text-input"), { key: "n" });
    expect(useComposeStore.getState().isOpen).toBe(false);
  });

  it("does not fire while a textarea is focused", () => {
    const { getByTestId } = render(<ShortcutHost />);
    fireEvent.keyDown(getByTestId("text-area"), { key: "n" });
    expect(useComposeStore.getState().isOpen).toBe(false);
  });

  it("does not fire on a contenteditable element", () => {
    const { getByTestId } = render(<ShortcutHost />);
    const editable = getByTestId("editable");
    // jsdom does not compute isContentEditable from the attribute
    Object.defineProperty(editable, "isContentEditable", { value: true });
    fireEvent.keyDown(editable, { key: "n" });
    expect(useComposeStore.getState().isOpen).toBe(false);
  });

  it("does not fire while the compose modal is open", () => {
    useComposeStore.setState({ isOpen: true, mode: "forward" });
    render(<ShortcutHost />);
    fireEvent.keyDown(document.body, { key: "r" });
    expect(useComposeStore.getState().mode).toBe("forward");
  });

  it("does not fire with modifier keys", () => {
    render(<ShortcutHost />);
    fireEvent.keyDown(document.body, { key: "n", metaKey: true });
    fireEvent.keyDown(document.body, { key: "n", ctrlKey: true });
    expect(useComposeStore.getState().isOpen).toBe(false);
  });

  it("opens reply for the selected mail on r", () => {
    useMailStore.setState({ selectedMail: makeMail() });
    render(<ShortcutHost />);
    fireEvent.keyDown(document.body, { key: "r" });
    const s = useComposeStore.getState();
    expect(s.isOpen).toBe(true);
    expect(s.mode).toBe("reply");
    expect(s.replyToMailId).toBe("m1");
  });

  it("falls back to the latest mail of the selected thread", () => {
    const thread: Thread = {
      thread_id: "t1",
      subject: "打ち合わせの件",
      last_date: "2026-07-10",
      mail_count: 2,
      from_addrs: ["tanaka@example.com"],
      mails: [makeMail("m1"), makeMail("m2")],
    };
    useMailStore.setState({ selectedThread: thread });
    render(<ShortcutHost />);
    fireEvent.keyDown(document.body, { key: "a" });
    const s = useComposeStore.getState();
    expect(s.isOpen).toBe(true);
    expect(s.mode).toBe("replyAll");
    expect(s.replyToMailId).toBe("m2");
  });

  it("opens forward on f", () => {
    useMailStore.setState({ selectedMail: makeMail() });
    render(<ShortcutHost />);
    fireEvent.keyDown(document.body, { key: "f" });
    expect(useComposeStore.getState().mode).toBe("forward");
  });

  it("does nothing on r/a/f when no mail is selected", () => {
    render(<ShortcutHost />);
    fireEvent.keyDown(document.body, { key: "r" });
    fireEvent.keyDown(document.body, { key: "a" });
    fireEvent.keyDown(document.body, { key: "f" });
    expect(useComposeStore.getState().isOpen).toBe(false);
  });
});
