import { describe, it, expect, beforeEach, vi } from "vitest";
import { useSearchStore, SEARCH_MODE_KEY } from "../stores/searchStore";
import type { SearchResult, Mail } from "../types/mail";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

function makeMail(id: string): Mail {
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
    is_flagged: false,
    fetched_at: "2026-07-12T00:00:00",
  };
}

function makeResults(ids: string[]): SearchResult[] {
  return ids.map((id) => ({
    mail: makeMail(id),
    project_id: null,
    project_name: null,
    snippet: "...",
  }));
}

describe("searchStore: selectedIndex navigation", () => {
  beforeEach(() => {
    useSearchStore.setState({
      query: "",
      results: [],
      searching: false,
      selectedIndex: -1,
    });
  });

  it("starts with no selection", () => {
    expect(useSearchStore.getState().selectedIndex).toBe(-1);
  });

  it("moveSelection(1) selects the first result when nothing is selected", () => {
    useSearchStore.setState({ results: makeResults(["a", "b", "c"]) });
    useSearchStore.getState().moveSelection(1);
    expect(useSearchStore.getState().selectedIndex).toBe(0);
  });

  it("moveSelection(1) advances to the next result", () => {
    useSearchStore.setState({
      results: makeResults(["a", "b", "c"]),
      selectedIndex: 0,
    });
    useSearchStore.getState().moveSelection(1);
    expect(useSearchStore.getState().selectedIndex).toBe(1);
  });

  it("moveSelection(-1) moves to the previous result", () => {
    useSearchStore.setState({
      results: makeResults(["a", "b", "c"]),
      selectedIndex: 1,
    });
    useSearchStore.getState().moveSelection(-1);
    expect(useSearchStore.getState().selectedIndex).toBe(0);
  });

  it("stops at the last result (no wrap)", () => {
    useSearchStore.setState({
      results: makeResults(["a", "b"]),
      selectedIndex: 1,
    });
    useSearchStore.getState().moveSelection(1);
    expect(useSearchStore.getState().selectedIndex).toBe(1);
  });

  it("stops at the first result (no wrap)", () => {
    useSearchStore.setState({
      results: makeResults(["a", "b"]),
      selectedIndex: 0,
    });
    useSearchStore.getState().moveSelection(-1);
    expect(useSearchStore.getState().selectedIndex).toBe(0);
  });

  it("does nothing when there are no results", () => {
    useSearchStore.setState({ results: [] });
    useSearchStore.getState().moveSelection(1);
    expect(useSearchStore.getState().selectedIndex).toBe(-1);
  });

  it("clearSearch resets selectedIndex", () => {
    useSearchStore.setState({
      results: makeResults(["a"]),
      selectedIndex: 0,
    });
    useSearchStore.getState().clearSearch();
    expect(useSearchStore.getState().selectedIndex).toBe(-1);
  });

  it("setSelectedIndex sets the index directly", () => {
    useSearchStore.setState({ results: makeResults(["a", "b"]) });
    useSearchStore.getState().setSelectedIndex(1);
    expect(useSearchStore.getState().selectedIndex).toBe(1);
  });
});

describe("search mode", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    useSearchStore.setState({
      mode: "fulltext",
      query: "",
      results: [],
      searching: false,
      selectedIndex: -1,
    });
    mockInvoke.mockResolvedValue([]);
  });

  it("デフォルトは fulltext で search_mails を呼ぶ", async () => {
    await useSearchStore.getState().search("acc1", "照明");
    expect(mockInvoke).toHaveBeenCalledWith("search_mails", {
      accountId: "acc1",
      query: "照明",
    });
  });

  it("semantic モードでは semantic_search を呼ぶ", async () => {
    useSearchStore.getState().setMode("semantic");
    await useSearchStore.getState().search("acc1", "照明");
    expect(mockInvoke).toHaveBeenCalledWith("semantic_search", {
      accountId: "acc1",
      query: "照明",
    });
  });

  it("setMode は localStorage に永続化する", () => {
    useSearchStore.getState().setMode("semantic");
    expect(localStorage.getItem(SEARCH_MODE_KEY)).toBe("semantic");
    useSearchStore.getState().setMode("fulltext");
    expect(localStorage.getItem(SEARCH_MODE_KEY)).toBe("fulltext");
  });

  it("不正な保存値は fulltext にフォールバックする", () => {
    localStorage.setItem(SEARCH_MODE_KEY, "garbage");
    expect(useSearchStore.getState().loadPersistedMode()).toBe("fulltext");
    localStorage.setItem(SEARCH_MODE_KEY, "semantic");
    expect(useSearchStore.getState().loadPersistedMode()).toBe("semantic");
  });
});

describe("search scope toggle passes projectId to search API", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSearchStore.setState({
      mode: "fulltext",
      query: "",
      results: [],
      searching: false,
      selectedIndex: -1,
      scopeToProject: false,
    });
    mockInvoke.mockResolvedValue([]);
  });

  it("scopeToProject が false のときは projectId なしで search_mails を呼ぶ", async () => {
    await useSearchStore.getState().search("acc1", "照明", "p1");
    expect(mockInvoke).toHaveBeenCalledWith("search_mails", {
      accountId: "acc1",
      query: "照明",
    });
  });

  it("scopeToProject が true のときは projectId 付きで search_mails を呼ぶ", async () => {
    useSearchStore.getState().setScopeToProject(true);
    await useSearchStore.getState().search("acc1", "照明", "p1");
    expect(mockInvoke).toHaveBeenCalledWith("search_mails", {
      accountId: "acc1",
      query: "照明",
      projectId: "p1",
    });
  });

  it("scopeToProject が true でも選択案件がなければ projectId は渡さない", async () => {
    useSearchStore.getState().setScopeToProject(true);
    await useSearchStore.getState().search("acc1", "照明", null);
    expect(mockInvoke).toHaveBeenCalledWith("search_mails", {
      accountId: "acc1",
      query: "照明",
    });
  });

  it("scopeToProject が true のときは semantic_search にも projectId を渡す", async () => {
    useSearchStore.getState().setMode("semantic");
    useSearchStore.getState().setScopeToProject(true);
    await useSearchStore.getState().search("acc1", "照明", "p1");
    expect(mockInvoke).toHaveBeenCalledWith("semantic_search", {
      accountId: "acc1",
      query: "照明",
      projectId: "p1",
    });
  });

  it("setScopeToProject はモード切替のような再検索を伴わずトグルするだけ", () => {
    useSearchStore.getState().setScopeToProject(true);
    expect(useSearchStore.getState().scopeToProject).toBe(true);
  });
});
