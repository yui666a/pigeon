import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock searchStore
const mockSearchStore = {
  query: "",
  results: [] as import("../types/mail").SearchResult[],
  searching: false,
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
    fetched_at: "2026-04-13T00:00:00",
    ...overrides,
  };
}

describe("SearchResults", () => {
  beforeEach(() => {
    mockSearchStore.query = "";
    mockSearchStore.results = [];
    mockSearchStore.searching = false;
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

  it("clears selectedThread and sets selectedMail on click", () => {
    const mail = makeMail({ subject: "Click Me" });
    const result: SearchResult = {
      mail,
      project_id: null,
      project_name: null,
      snippet: "...",
    };
    mockSearchStore.query = "click";
    mockSearchStore.results = [result];
    render(<SearchResults />);

    fireEvent.click(screen.getByText("Click Me"));

    // Must clear thread first to prevent MailView from showing stale MailTabs
    expect(mockSelectThread).toHaveBeenCalledWith(null);
    expect(mockSelectMail).toHaveBeenCalledWith(mail);
    // selectThread(null) must be called before selectMail
    const threadCallOrder = mockSelectThread.mock.invocationCallOrder[0];
    const mailCallOrder = mockSelectMail.mock.invocationCallOrder[0];
    expect(threadCallOrder).toBeLessThan(mailCallOrder);
  });
});
