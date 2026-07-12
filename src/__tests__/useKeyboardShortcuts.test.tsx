import { render, fireEvent, screen } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useKeyboardShortcuts } from "../hooks/useKeyboardShortcuts";
import { useMailStore } from "../stores/mailStore";
import { useComposeStore } from "../stores/composeStore";
import type { Mail } from "../types/mail";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

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
    date: "2026-07-12T10:00:00",
    has_attachments: false,
    raw_size: null,
    uid: 1,
    flags: null,
    is_read: true,
    fetched_at: "2026-07-12T00:00:00",
  };
}

function Harness() {
  useKeyboardShortcuts();
  return <input aria-label="field" />;
}

describe("useKeyboardShortcuts: e = archive", () => {
  const archiveMail = vi.fn();

  beforeEach(() => {
    archiveMail.mockReset();
    useComposeStore.setState({ isOpen: false });
    useMailStore.setState({
      selectedMail: null,
      selectedThread: null,
      archiveMail,
    });
  });

  it("archives the selected mail on 'e'", () => {
    const mail = makeMail("m1");
    useMailStore.setState({ selectedMail: mail });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "e" });

    expect(archiveMail).toHaveBeenCalledWith(
      expect.objectContaining({ id: "m1" }),
    );
  });

  it("archives the latest mail of the selected thread when no mail is selected", () => {
    const m1 = makeMail("m1");
    const m2 = makeMail("m2");
    useMailStore.setState({
      selectedThread: {
        thread_id: "t1",
        subject: "S",
        last_date: m2.date,
        mail_count: 2,
        from_addrs: [],
        mails: [m1, m2],
      },
    });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "e" });

    expect(archiveMail).toHaveBeenCalledWith(
      expect.objectContaining({ id: "m2" }),
    );
  });

  it("does nothing when no mail is targeted", () => {
    render(<Harness />);
    fireEvent.keyDown(window, { key: "e" });
    expect(archiveMail).not.toHaveBeenCalled();
  });

  it("does nothing while typing in a text input", () => {
    useMailStore.setState({ selectedMail: makeMail("m1") });
    render(<Harness />);

    fireEvent.keyDown(screen.getByLabelText("field"), { key: "e" });

    expect(archiveMail).not.toHaveBeenCalled();
  });

  it("does nothing while compose is open", () => {
    useMailStore.setState({ selectedMail: makeMail("m1") });
    useComposeStore.setState({ isOpen: true });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "e" });

    expect(archiveMail).not.toHaveBeenCalled();
  });

  it("does nothing when a modifier key is held", () => {
    useMailStore.setState({ selectedMail: makeMail("m1") });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "e", metaKey: true });

    expect(archiveMail).not.toHaveBeenCalled();
  });
});
