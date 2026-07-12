import { describe, it, expect, beforeEach, vi } from "vitest";
import { useSearchStore } from "../stores/searchStore";
import type { SearchResult, Mail } from "../types/mail";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
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
