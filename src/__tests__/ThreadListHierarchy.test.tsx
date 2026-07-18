import { render, screen, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ThreadList } from "../components/thread-list/ThreadList";
import { useAccountStore } from "../stores/accountStore";
import { useMailStore } from "../stores/mailStore";
import { useProjectStore } from "../stores/projectStore";
import { useSelectionStore } from "../stores/selectionStore";
import type { Project } from "../types/project";
import type { Thread } from "../types/mail";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

const p = (id: string, name: string, parent: string | null): Project => ({
  id, account_id: "acc1", name, description: null, color: null,
  is_archived: false, parent_id: parent,
  created_at: "2026-07-18", updated_at: "2026-07-18",
});

function makeThread(overrides: Partial<Thread> = {}): Thread {
  return {
    thread_id: "<thread-1@example.com>",
    subject: "テストスレッド",
    last_date: "2026-07-18T10:00:00+09:00",
    mail_count: 1,
    from_addrs: ["alice@example.com"],
    mails: [],
    projects: [],
    ...overrides,
  };
}

describe("ThreadList hierarchy display", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockImplementation((command: unknown) => {
      if (command === "get_unread_counts") {
        return Promise.resolve({ by_project: {}, unclassified: 0 });
      }
      return Promise.resolve([]);
    });
    useAccountStore.setState({ selectedAccountId: "acc1" });
    useProjectStore.setState({ selectedProjectId: null, projects: [] });
    useMailStore.setState({
      threads: [],
      selectedThread: null,
      selectedMail: null,
      syncing: false,
      needsReauth: false,
      syncProgress: null,
    });
    useSelectionStore.getState().clear();
  });

  it("shows a breadcrumb for the selected project in the header", async () => {
    const projects = [p("root", "埼玉", null), p("leaf", "音響", "root")];
    useProjectStore.setState({ selectedProjectId: "leaf", projects });
    mockInvoke.mockImplementation((command: unknown) => {
      if (command === "get_unread_counts") {
        return Promise.resolve({ by_project: {}, unclassified: 0 });
      }
      if (command === "get_threads_by_project") {
        return Promise.resolve([makeThread()]);
      }
      return Promise.resolve([]);
    });

    render(<ThreadList viewMode="project" />);
    await act(async () => {});

    expect(screen.getByText("埼玉 > 音響")).toBeInTheDocument();
  });

  it("shows relative path chips for threads from descendant projects", async () => {
    const projects = [p("root", "埼玉", null), p("leaf", "音響", "root")];
    useProjectStore.setState({ selectedProjectId: "root", projects });
    mockInvoke.mockImplementation((command: unknown) => {
      if (command === "get_unread_counts") {
        return Promise.resolve({ by_project: {}, unclassified: 0 });
      }
      if (command === "get_threads_by_project") {
        return Promise.resolve([
          makeThread({
            projects: [{ project_id: "leaf", display_path: "埼玉 > 音響" }],
          }),
        ]);
      }
      return Promise.resolve([]);
    });

    render(<ThreadList viewMode="project" />);
    await act(async () => {});

    expect(screen.getByText("埼玉 > 音響")).toBeInTheDocument();
  });

  it("does not show a breadcrumb when no project is selected", async () => {
    useMailStore.setState({ threads: [makeThread()] });

    render(<ThreadList viewMode="threads" />);
    await act(async () => {});

    expect(screen.queryByRole("navigation", { name: "案件のパンくずリスト" })).not.toBeInTheDocument();
  });
});
