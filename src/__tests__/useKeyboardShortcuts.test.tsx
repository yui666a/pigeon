import { render, fireEvent, screen } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useKeyboardShortcuts } from "../hooks/useKeyboardShortcuts";
import { SEARCH_INPUT_ID } from "../components/sidebar/SearchBar";
import { useMailStore } from "../stores/mailStore";
import { useComposeStore } from "../stores/composeStore";
import { useUiStore } from "../stores/uiStore";
import { useSearchStore } from "../stores/searchStore";
import type { Mail, Thread, SearchResult } from "../types/mail";

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

function makeThread(threadId: string, mails: Mail[]): Thread {
  return {
    thread_id: threadId,
    subject: "S",
    last_date: mails[mails.length - 1].date,
    mail_count: mails.length,
    from_addrs: [],
    mails,
  };
}

function Harness() {
  useKeyboardShortcuts();
  return (
    <div>
      <input aria-label="field" />
      <input id={SEARCH_INPUT_ID} aria-label="search" />
    </div>
  );
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

describe("useKeyboardShortcuts: j/k = mail navigation", () => {
  const selectMail = vi.fn();
  const selectThread = vi.fn();

  beforeEach(() => {
    selectMail.mockReset();
    selectThread.mockReset();
    useComposeStore.setState({ isOpen: false });
    useUiStore.setState({ viewMode: "threads" });
    useMailStore.setState({
      selectedMail: null,
      selectedThread: null,
      threads: [],
      selectMail,
      selectThread,
    });
  });

  it("selects the next mail in the thread on 'j'", () => {
    const mails = [makeMail("m1"), makeMail("m2"), makeMail("m3")];
    useMailStore.setState({
      selectedThread: makeThread("t1", mails),
      selectedMail: mails[1],
    });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "j" });

    expect(selectMail).toHaveBeenCalledWith(
      expect.objectContaining({ id: "m3" }),
    );
  });

  it("selects the previous mail in the thread on 'k'", () => {
    const mails = [makeMail("m1"), makeMail("m2"), makeMail("m3")];
    useMailStore.setState({
      selectedThread: makeThread("t1", mails),
      selectedMail: mails[1],
    });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "k" });

    expect(selectMail).toHaveBeenCalledWith(
      expect.objectContaining({ id: "m1" }),
    );
  });

  it("stops at the last mail (no wrap) on 'j'", () => {
    const mails = [makeMail("m1"), makeMail("m2")];
    useMailStore.setState({
      selectedThread: makeThread("t1", mails),
      selectedMail: mails[1],
    });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "j" });

    expect(selectMail).not.toHaveBeenCalled();
  });

  it("stops at the first mail (no wrap) on 'k'", () => {
    const mails = [makeMail("m1"), makeMail("m2")];
    useMailStore.setState({
      selectedThread: makeThread("t1", mails),
      selectedMail: mails[0],
    });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "k" });

    expect(selectMail).not.toHaveBeenCalled();
  });

  it("treats the displayed latest mail as current when no mail is selected", () => {
    // スレッド選択直後は末尾（最新）メールが表示されている
    const mails = [makeMail("m1"), makeMail("m2")];
    useMailStore.setState({ selectedThread: makeThread("t1", mails) });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "k" });
    expect(selectMail).toHaveBeenCalledWith(
      expect.objectContaining({ id: "m1" }),
    );

    selectMail.mockReset();
    fireEvent.keyDown(window, { key: "j" });
    expect(selectMail).not.toHaveBeenCalled();
  });

  it("selects the first thread on 'j' when no thread is selected", () => {
    const threads = [
      makeThread("t1", [makeMail("m1")]),
      makeThread("t2", [makeMail("m2")]),
    ];
    useMailStore.setState({ threads });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "j" });

    expect(selectThread).toHaveBeenCalledWith(
      expect.objectContaining({ thread_id: "t1" }),
    );
    expect(selectMail).not.toHaveBeenCalled();
  });

  it("does nothing on 'k' when no thread is selected (no previous)", () => {
    useMailStore.setState({ threads: [makeThread("t1", [makeMail("m1")])] });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "k" });

    expect(selectThread).not.toHaveBeenCalled();
    expect(selectMail).not.toHaveBeenCalled();
  });

  it("does nothing when there are no threads at all", () => {
    render(<Harness />);
    fireEvent.keyDown(window, { key: "j" });
    expect(selectThread).not.toHaveBeenCalled();
    expect(selectMail).not.toHaveBeenCalled();
  });

  it("does nothing while typing in a text input", () => {
    const mails = [makeMail("m1"), makeMail("m2")];
    useMailStore.setState({
      selectedThread: makeThread("t1", mails),
      selectedMail: mails[0],
    });
    render(<Harness />);

    fireEvent.keyDown(screen.getByLabelText("field"), { key: "j" });

    expect(selectMail).not.toHaveBeenCalled();
  });

  it("does nothing while compose is open", () => {
    const mails = [makeMail("m1"), makeMail("m2")];
    useMailStore.setState({
      selectedThread: makeThread("t1", mails),
      selectedMail: mails[0],
    });
    useComposeStore.setState({ isOpen: true });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "j" });

    expect(selectMail).not.toHaveBeenCalled();
  });

  it("does nothing when a modifier key is held", () => {
    const mails = [makeMail("m1"), makeMail("m2")];
    useMailStore.setState({
      selectedThread: makeThread("t1", mails),
      selectedMail: mails[0],
    });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "j", ctrlKey: true });

    expect(selectMail).not.toHaveBeenCalled();
  });
});

describe("useKeyboardShortcuts: j/k = search results navigation", () => {
  function makeSearchMail(id: string): Mail {
    return {
      id,
      account_id: "acc1",
      folder: "INBOX",
      message_id: `<${id}@ex.com>`,
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

  function makeSearchResults(ids: string[]): SearchResult[] {
    return ids.map((id) => ({
      mail: makeSearchMail(id),
      project_id: null,
      project_name: null,
      snippet: "...",
    }));
  }

  beforeEach(() => {
    useComposeStore.setState({ isOpen: false });
    useUiStore.setState({ viewMode: "search" });
    useSearchStore.setState({
      results: makeSearchResults(["m1", "m2", "m3"]),
      selectedIndex: 1,
    });
    // スレッド内ナビが誤って動いていないことも確認できるようにしておく
    useMailStore.setState({ selectedMail: null, selectedThread: null });
  });

  it("moves the search selection forward on 'j' while the search view is active", () => {
    render(<Harness />);

    fireEvent.keyDown(window, { key: "j" });

    expect(useSearchStore.getState().selectedIndex).toBe(2);
  });

  it("moves the search selection backward on 'k' while the search view is active", () => {
    render(<Harness />);

    fireEvent.keyDown(window, { key: "k" });

    expect(useSearchStore.getState().selectedIndex).toBe(0);
  });

  it("stops at the last result (no wrap) on 'j'", () => {
    useSearchStore.setState({ selectedIndex: 2 });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "j" });

    expect(useSearchStore.getState().selectedIndex).toBe(2);
  });

  it("stops at the first result (no wrap) on 'k'", () => {
    useSearchStore.setState({ selectedIndex: 0 });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "k" });

    expect(useSearchStore.getState().selectedIndex).toBe(0);
  });

  it("selects the first result on 'j' when nothing is selected yet", () => {
    useSearchStore.setState({ selectedIndex: -1 });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "j" });

    expect(useSearchStore.getState().selectedIndex).toBe(0);
  });

  it("does not touch thread navigation while the search view is active", () => {
    const mails = [makeSearchMail("t1"), makeSearchMail("t2")];
    const selectMail = vi.fn();
    useMailStore.setState({
      selectedThread: {
        thread_id: "t1",
        subject: "S",
        last_date: mails[1].date,
        mail_count: 2,
        from_addrs: [],
        mails,
      },
      selectedMail: mails[0],
      selectMail,
    });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "j" });

    expect(selectMail).not.toHaveBeenCalled();
    expect(useSearchStore.getState().selectedIndex).toBe(2);
  });

  it("does nothing while typing in a text input", () => {
    render(<Harness />);

    fireEvent.keyDown(screen.getByLabelText("field"), { key: "j" });

    expect(useSearchStore.getState().selectedIndex).toBe(1);
  });

  it("falls back to thread navigation when the search view is not active", () => {
    useUiStore.setState({ viewMode: "threads" });
    const mails = [makeSearchMail("t1"), makeSearchMail("t2")];
    const selectMail = vi.fn();
    useMailStore.setState({
      selectedThread: {
        thread_id: "t1",
        subject: "S",
        last_date: mails[1].date,
        mail_count: 2,
        from_addrs: [],
        mails,
      },
      selectedMail: mails[0],
      selectMail,
    });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "j" });

    expect(selectMail).toHaveBeenCalledWith(
      expect.objectContaining({ id: "t2" }),
    );
    expect(useSearchStore.getState().selectedIndex).toBe(1);
  });
});

describe("useKeyboardShortcuts: / = focus search", () => {
  beforeEach(() => {
    useComposeStore.setState({ isOpen: false });
    useUiStore.setState({ viewMode: "threads" });
    useMailStore.setState({ selectedMail: null, selectedThread: null });
  });

  it("focuses the search input and prevents default on '/'", () => {
    render(<Harness />);

    // fireEvent は preventDefault されると false を返す
    const notPrevented = fireEvent.keyDown(window, { key: "/" });

    expect(notPrevented).toBe(false);
    expect(screen.getByLabelText("search")).toHaveFocus();
  });

  it("does nothing while typing in another text input", () => {
    render(<Harness />);
    const field = screen.getByLabelText("field");
    field.focus();

    const notPrevented = fireEvent.keyDown(field, { key: "/" });

    expect(notPrevented).toBe(true);
    expect(field).toHaveFocus();
  });

  it("does nothing while compose is open", () => {
    useComposeStore.setState({ isOpen: true });
    render(<Harness />);

    fireEvent.keyDown(window, { key: "/" });

    expect(screen.getByLabelText("search")).not.toHaveFocus();
  });
});
