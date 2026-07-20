import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ProjectTree } from "../components/sidebar/ProjectTree";
import { useAccountStore } from "../stores/accountStore";
import { useProjectStore } from "../stores/projectStore";
import { useMailStore } from "../stores/mailStore";
import { useDragStore } from "../stores/dragStore";
import { useErrorStore } from "../stores/errorStore";
import type { Project } from "../types/project";
import type { Mail, Thread } from "../types/mail";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

const project: Project = {
  id: "p1",
  account_id: "acc1",
  name: "案件A",
  description: null,
  color: "#6b7280",
  is_archived: false,
  parent_id: null,
  created_at: "",
  updated_at: "",
};

const mail = (id: string): Mail => ({
  id,
  account_id: "acc1",
  folder: "INBOX",
  message_id: `<${id}@example.com>`,
  in_reply_to: null,
  references: null,
  from_addr: "a@example.com",
  to_addr: "b@example.com",
  cc_addr: null,
  subject: `件名 ${id}`,
  body_text: "本文",
  body_html: null,
  date: "2026-07-13T00:00:00Z",
  has_attachments: false,
  raw_size: null,
  uid: 1,
  flags: null,
  is_read: false,
  is_flagged: false,
  fetched_at: "2026-07-13T00:00:00Z",
});

const threadOf = (m: Mail): Thread => ({
  thread_id: m.message_id,
  subject: m.subject,
  last_date: m.date,
  mail_count: 1,
  from_addrs: [m.from_addr],
  mails: [m],
  projects: [],
});

const unclassified = [mail("m1"), mail("m2")];

describe("ProjectTree drop", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useAccountStore.setState({ selectedAccountId: "acc1" });
    useProjectStore.setState({
      projects: [project],
      selectedProjectId: null,
      loading: false,
      directories: {},
      contexts: {},
      scanningProjects: {},
    });
    useMailStore.setState({
      unclassifiedMails: unclassified,
      unclassifiedThreads: unclassified.map(threadOf),
      unreadCounts: { by_project: {}, unclassified: 0 },
    });
    useDragStore.setState({ draggingMailIds: ["m1", "m2"], dragLabel: "件名 m1" });
    useErrorStore.setState({ toasts: [] });

    mockInvoke.mockImplementation((cmd: unknown) => {
      switch (cmd) {
        case "get_projects":
          return Promise.resolve([project]);
        case "get_project_directory":
          return Promise.resolve(null);
        case "get_unclassified_threads":
          return Promise.resolve({ threads: unclassified.map(threadOf), has_more: false });
        case "get_unread_counts":
          return Promise.resolve({ by_project: {}, unclassified: 2 });
        case "bulk_move_mails":
          return Promise.resolve({ succeeded: ["m1", "m2"], failed: [] });
        default:
          return Promise.resolve(null);
      }
    });
  });

  const drop = () => {
    render(<ProjectTree onSelectUnclassified={() => {}} onSelectProject={() => {}} />);
    fireEvent.mouseUp(screen.getByText("案件A"));
  };

  it("moves all dragged mails with a single bulk_move_mails invoke", async () => {
    drop();

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("bulk_move_mails", {
        mailIds: ["m1", "m2"],
        projectId: "p1",
      });
    });
    expect(
      mockInvoke.mock.calls.filter((c) => c[0] === "bulk_move_mails"),
    ).toHaveLength(1);
    expect(mockInvoke).not.toHaveBeenCalledWith("move_mail", expect.anything());
  });

  it("ends the drag and removes succeeded mails from the unclassified list", async () => {
    drop();

    expect(useDragStore.getState().draggingMailIds).toBeNull();
    await waitFor(() => {
      expect(useMailStore.getState().unclassifiedMails).toHaveLength(0);
      expect(useMailStore.getState().unclassifiedThreads).toHaveLength(0);
    });
  });

  it("keeps failed mails in the unclassified list on partial failure", async () => {
    mockInvoke.mockImplementation((cmd: unknown) => {
      switch (cmd) {
        case "get_projects":
          return Promise.resolve([project]);
        case "get_project_directory":
          return Promise.resolve(null);
        case "get_unclassified_threads":
          return Promise.resolve({ threads: unclassified.map(threadOf), has_more: false });
        case "get_unread_counts":
          return Promise.resolve({ by_project: {}, unclassified: 2 });
        case "bulk_move_mails":
          return Promise.resolve({ succeeded: ["m1"], failed: [["m2", "boom"]] });
        default:
          return Promise.resolve(null);
      }
    });

    drop();

    await waitFor(() => {
      expect(useMailStore.getState().unclassifiedMails.map((m) => m.id)).toEqual([
        "m2",
      ]);
    });
    // 一部失敗はエラートーストで通知される（bulkMoveMails の共通レポート経由）
    expect(useErrorStore.getState().toasts.some((t) => t.kind === "error")).toBe(true);
  });
});
