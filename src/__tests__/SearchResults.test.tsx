import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock searchStore
const mockSetSelectedIndex = vi.fn();
const mockSearchStore = {
  query: "",
  results: [] as import("../types/mail").SearchResult[],
  searching: false,
  selectedIndex: -1,
  setSelectedIndex: mockSetSelectedIndex,
};
vi.mock("../stores/searchStore", () => ({
  useSearchStore: (selector: (s: typeof mockSearchStore) => unknown) =>
    selector(mockSearchStore),
}));

// Mock mailStore — track calls to selectThread and selectMail
const mockSelectThread = vi.fn();
const mockSelectMail = vi.fn();
vi.mock("../stores/mailStore", () => ({
  useMailStore: (selector: (s: Record<string, unknown>) => unknown) =>
    selector({
      selectThread: mockSelectThread,
      selectMail: mockSelectMail,
    }),
}));

import { SearchResults } from "../components/thread-list/SearchResults";
import type { SearchResult, Mail } from "../types/mail";

function makeMail(overrides: Partial<Mail> = {}): Mail {
  return {
    id: "m1",
    account_id: "acc1",
    folder: "INBOX",
    message_id: "<msg1@ex.com>",
    in_reply_to: null,
    references: null,
    from_addr: "sender@example.com",
    to_addr: "me@example.com",
    cc_addr: null,
    subject: "Test Subject",
    body_text: "Test body",
    body_html: null,
    date: "2026-04-13T10:00:00",
    has_attachments: false,
    raw_size: null,
    uid: 1,
    flags: null,
    is_read: false,
    fetched_at: "2026-04-13T00:00:00",
    ...overrides,
  };
}

describe("SearchResults", () => {
  beforeEach(() => {
    mockSearchStore.query = "";
    mockSearchStore.results = [];
    mockSearchStore.searching = false;
    mockSearchStore.selectedIndex = -1;
    vi.clearAllMocks();
  });

  it("shows loading state", () => {
    mockSearchStore.searching = true;
    mockSearchStore.query = "test";
    render(<SearchResults />);
    expect(screen.getByText("検索中...")).toBeInTheDocument();
  });

  it("shows empty state when no results", () => {
    mockSearchStore.query = "nonexistent";
    mockSearchStore.results = [];
    render(<SearchResults />);
    expect(screen.getByText(/検索結果がありません/)).toBeInTheDocument();
  });

  it("renders search results", () => {
    const result: SearchResult = {
      mail: makeMail({ subject: "見積もりの件" }),
      project_id: "proj1",
      project_name: "案件A",
      snippet: "...<b>見積もり</b>について...",
    };
    mockSearchStore.query = "見積もり";
    mockSearchStore.results = [result];
    render(<SearchResults />);
    expect(screen.getByText("見積もりの件")).toBeInTheDocument();
    expect(screen.getByText("案件A")).toBeInTheDocument();
  });

  it("shows unclassified label when no project", () => {
    const result: SearchResult = {
      mail: makeMail({ subject: "Orphan" }),
      project_id: null,
      project_name: null,
      snippet: "...",
    };
    mockSearchStore.query = "orphan";
    mockSearchStore.results = [result];
    render(<SearchResults />);
    expect(screen.getByText("未分類")).toBeInTheDocument();
  });

  it("sanitizes dangerous HTML in snippets", () => {
    const result: SearchResult = {
      mail: makeMail({ subject: "XSS test" }),
      project_id: null,
      project_name: null,
      snippet: '<b>safe</b><script>alert("xss")</script>',
    };
    mockSearchStore.query = "xss";
    mockSearchStore.results = [result];
    const { container } = render(<SearchResults />);
    // <script> should be stripped by DOMPurify
    expect(container.querySelector("script")).toBeNull();
    // <b> should be preserved
    expect(container.querySelector("b")?.textContent).toBe("safe");
  });

  it("clears selectedThread and sets selectedMail when selectedIndex is set", () => {
    // クリックは setSelectedIndex を呼び、右ペインへの反映は selectedIndex の
    // 変化を監視する effect が担う（j/k ナビと同じ経路に統一するため）
    const mail = makeMail({ subject: "Click Me" });
    const result: SearchResult = {
      mail,
      project_id: null,
      project_name: null,
      snippet: "...",
    };
    mockSearchStore.query = "click";
    mockSearchStore.results = [result];
    mockSearchStore.selectedIndex = 0;
    render(<SearchResults />);

    // Must clear thread first to prevent MailView from showing stale MailTabs
    expect(mockSelectThread).toHaveBeenCalledWith(null);
    expect(mockSelectMail).toHaveBeenCalledWith(mail);
    // selectThread(null) must be called before selectMail
    const threadCallOrder = mockSelectThread.mock.invocationCallOrder[0];
    const mailCallOrder = mockSelectMail.mock.invocationCallOrder[0];
    expect(threadCallOrder).toBeLessThan(mailCallOrder);
  });

  it("sets selectedIndex on click for j/k nav to resume from there", () => {
    const results: SearchResult[] = [
      {
        mail: makeMail({ id: "m1", subject: "First" }),
        project_id: null,
        project_name: null,
        snippet: "...",
      },
      {
        mail: makeMail({ id: "m2", subject: "Second" }),
        project_id: null,
        project_name: null,
        snippet: "...",
      },
    ];
    mockSearchStore.query = "x";
    mockSearchStore.results = results;
    render(<SearchResults />);

    fireEvent.click(screen.getByText("Second"));

    expect(mockSetSelectedIndex).toHaveBeenCalledWith(1);
  });

  it("highlights the selected row", () => {
    const results: SearchResult[] = [
      {
        mail: makeMail({ id: "m1", subject: "First" }),
        project_id: null,
        project_name: null,
        snippet: "...",
      },
      {
        mail: makeMail({ id: "m2", subject: "Second" }),
        project_id: null,
        project_name: null,
        snippet: "...",
      },
    ];
    mockSearchStore.query = "x";
    mockSearchStore.results = results;
    mockSearchStore.selectedIndex = 1;
    render(<SearchResults />);

    const selected = screen.getByText("Second").closest("button");
    const notSelected = screen.getByText("First").closest("button");
    expect(selected?.getAttribute("aria-selected")).toBe("true");
    expect(notSelected?.getAttribute("aria-selected")).toBe("false");
  });

  it("selecting via j/k reflects to the right pane (selectThread(null) then selectMail)", () => {
    const results: SearchResult[] = [
      {
        mail: makeMail({ id: "m1", subject: "First" }),
        project_id: null,
        project_name: null,
        snippet: "...",
      },
      {
        mail: makeMail({ id: "m2", subject: "Second" }),
        project_id: null,
        project_name: null,
        snippet: "...",
      },
    ];
    mockSearchStore.query = "x";
    mockSearchStore.results = results;
    mockSearchStore.selectedIndex = 1;
    render(<SearchResults />);

    expect(mockSelectThread).toHaveBeenCalledWith(null);
    expect(mockSelectMail).toHaveBeenCalledWith(results[1].mail);
    const threadCallOrder = mockSelectThread.mock.invocationCallOrder[0];
    const mailCallOrder = mockSelectMail.mock.invocationCallOrder[0];
    expect(threadCallOrder).toBeLessThan(mailCallOrder);
  });
});
