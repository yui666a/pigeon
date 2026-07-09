import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { Sidebar } from "../components/sidebar/Sidebar";
import { useAccountStore } from "../stores/accountStore";
import { useProjectStore } from "../stores/projectStore";
import type { Account } from "../types/account";
import type { Project } from "../types/project";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(),
}));

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

import { open } from "@tauri-apps/plugin-dialog";

const account: Account = {
  id: "acc1",
  name: "Test Account",
  email: "test@example.com",
  imap_host: "imap.example.com",
  imap_port: 993,
  smtp_host: "smtp.example.com",
  smtp_port: 587,
  auth_type: "plain",
  provider: "other",
  needs_reauth: false,
  created_at: "",
};

const project: Project = {
  id: "p1",
  account_id: "acc1",
  name: "新規案件",
  description: null,
  color: "#6b7280",
  is_archived: false,
  created_at: "",
  updated_at: "",
};

describe("Sidebar - handleProjectSubmit", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useAccountStore.setState({
      accounts: [account],
      selectedAccountId: "acc1",
      loading: false,
      error: null,
      oauthStatus: "idle",
      oauthError: null,
      reauthAccountId: null,
    });
    useProjectStore.setState({
      projects: [],
      selectedProjectId: null,
      loading: false,
      error: null,
      directories: {},
      contexts: {},
      scanningProjects: {},
    });

    mockInvoke.mockImplementation((cmd: unknown) => {
      switch (cmd) {
        case "get_accounts":
          return Promise.resolve([account]);
        case "get_projects":
          return Promise.resolve([]);
        case "get_project_directory":
          return Promise.resolve(null);
        case "get_unclassified_mails":
          return Promise.resolve([]);
        case "create_project":
          return Promise.resolve(project);
        case "link_project_directory":
          return Promise.reject("Directory error");
        default:
          return Promise.resolve(null);
      }
    });
  });

  it("closes the project form even when linkDirectory fails, and does not double-create the project", async () => {
    vi.mocked(open).mockResolvedValue("/tmp/some-dir");

    render(<Sidebar />);

    fireEvent.click(await screen.findByText("+ 案件を作成"));

    fireEvent.change(screen.getByPlaceholderText("案件名を入力"), {
      target: { value: "新規案件" },
    });

    fireEvent.click(screen.getByRole("button", { name: /フォルダを選択/ }));
    await screen.findByText("/tmp/some-dir");

    fireEvent.submit(screen.getByRole("button", { name: "作成" }).closest("form")!);

    // Form should close (project name input disappears) despite link failure
    await waitFor(() => {
      expect(screen.queryByPlaceholderText("案件名を入力")).not.toBeInTheDocument();
    });

    expect(mockInvoke).toHaveBeenCalledWith(
      "create_project",
      expect.objectContaining({ accountId: "acc1", name: "新規案件" }),
    );
    expect(
      mockInvoke.mock.calls.filter((call) => call[0] === "create_project"),
    ).toHaveLength(1);
  });
});
