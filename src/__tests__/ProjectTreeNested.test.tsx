import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ProjectTree } from "../components/sidebar/ProjectTree";
import { useAccountStore } from "../stores/accountStore";
import { useProjectStore } from "../stores/projectStore";
import { useMailStore } from "../stores/mailStore";
import { useErrorStore } from "../stores/errorStore";
import type { Project } from "../types/project";

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

const root: Project = {
  id: "root",
  account_id: "acc1",
  name: "ルート案件",
  description: null,
  color: "#6b7280",
  is_archived: false,
  parent_id: null,
  created_at: "",
  updated_at: "",
};

const mid: Project = {
  id: "mid",
  account_id: "acc1",
  name: "中間案件",
  description: null,
  color: "#6b7280",
  is_archived: false,
  parent_id: "root",
  created_at: "",
  updated_at: "",
};

function setupStores(unreadByProject: Record<string, number> = {}) {
  useAccountStore.setState({ selectedAccountId: "acc1" });
  useProjectStore.setState({
    projects: [root, mid],
    selectedProjectId: null,
    loading: false,
    directories: {},
    contexts: {},
    scanningProjects: {},
    expandedIds: new Set(["root"]),
  });
  useMailStore.setState({
    unclassifiedMails: [],
    unclassifiedThreads: [],
    unreadCounts: { by_project: unreadByProject, unclassified: 0 },
  });
  useErrorStore.setState({ toasts: [] });

  mockInvoke.mockImplementation((cmd: unknown) => {
    switch (cmd) {
      case "get_projects":
        return Promise.resolve([root, mid]);
      case "get_project_directory":
        return Promise.resolve(null);
      case "get_unclassified_threads":
        return Promise.resolve([]);
      case "get_unread_counts":
        return Promise.resolve({ by_project: unreadByProject, unclassified: 0 });
      default:
        return Promise.resolve(null);
    }
  });
}

describe("ProjectTree nested rendering", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setupStores();
  });

  it("renders children indented under expanded parent and hides when collapsed", async () => {
    render(<ProjectTree onSelectUnclassified={() => {}} onSelectProject={() => {}} />);

    await waitFor(() => {
      expect(screen.getByText("ルート案件")).toBeInTheDocument();
    });
    // 初期状態（root が展開済み）では mid が見える
    expect(screen.getByText("中間案件")).toBeInTheDocument();

    // シェブロンをクリックすると mid が消える
    fireEvent.click(screen.getByLabelText("ルート案件を折りたたむ"));
    await waitFor(() => {
      expect(screen.queryByText("中間案件")).not.toBeInTheDocument();
    });
  });

  it("shows aggregated unread badge on parent", async () => {
    setupStores({ root: 1, mid: 2 });
    render(<ProjectTree onSelectUnclassified={() => {}} onSelectProject={() => {}} />);

    await waitFor(() => {
      expect(screen.getByText("ルート案件")).toBeInTheDocument();
    });
    // root の直接未読は1だが、mid(2)を合算した3が表示される
    expect(screen.getByTitle("未読 3 件")).toHaveTextContent("3");
    expect(screen.getByTitle("未読 2 件")).toHaveTextContent("2");
  });

  it("move dialog disables self and descendants", async () => {
    render(<ProjectTree onSelectUnclassified={() => {}} onSelectProject={() => {}} />);

    await waitFor(() => {
      expect(screen.getByText("ルート案件")).toBeInTheDocument();
    });

    fireEvent.contextMenu(screen.getByText("ルート案件"));
    fireEvent.click(await screen.findByText("親を変更..."));

    const dialog = await screen.findByRole("dialog", { name: /親を変更/ });
    const rootOption = within(dialog).getByRole("radio", { name: "ルート案件" });
    const midOption = within(dialog).getByRole("radio", { name: "中間案件" });

    expect(rootOption).toBeDisabled();
    expect(midOption).toBeDisabled();
  });

  it("親変更が失敗してもunhandled rejectionにならずダイアログは開いたままになる", async () => {
    mockInvoke.mockImplementation((cmd: unknown) => {
      switch (cmd) {
        case "get_projects":
          return Promise.resolve([root, mid]);
        case "get_project_directory":
          return Promise.resolve(null);
        case "get_unclassified_threads":
          return Promise.resolve([]);
        case "get_unread_counts":
          return Promise.resolve({ by_project: {}, unclassified: 0 });
        case "set_project_parent":
          return Promise.reject(new Error("failed"));
        default:
          return Promise.resolve(null);
      }
    });

    render(<ProjectTree onSelectUnclassified={() => {}} onSelectProject={() => {}} />);
    await waitFor(() => {
      expect(screen.getByText("中間案件")).toBeInTheDocument();
    });

    fireEvent.contextMenu(screen.getByText("中間案件"));
    fireEvent.click(await screen.findByText("親を変更..."));
    const dialog = await screen.findByRole("dialog", { name: /親を変更/ });
    fireEvent.click(within(dialog).getByRole("radio", { name: "ルート（親なし）" }));
    fireEvent.click(within(dialog).getByText("変更"));

    await waitFor(() => {
      expect(useErrorStore.getState().toasts.length).toBeGreaterThan(0);
    });
    // 失敗時はダイアログを閉じない
    expect(screen.getByRole("dialog", { name: /親を変更/ })).toBeInTheDocument();
  });

  it("子案件作成が失敗してもunhandled rejectionにならずフォームは開いたままになる", async () => {
    mockInvoke.mockImplementation((cmd: unknown) => {
      switch (cmd) {
        case "get_projects":
          return Promise.resolve([root, mid]);
        case "get_project_directory":
          return Promise.resolve(null);
        case "get_unclassified_threads":
          return Promise.resolve([]);
        case "get_unread_counts":
          return Promise.resolve({ by_project: {}, unclassified: 0 });
        case "create_project":
          return Promise.reject(new Error("failed"));
        default:
          return Promise.resolve(null);
      }
    });

    render(<ProjectTree onSelectUnclassified={() => {}} onSelectProject={() => {}} />);
    await waitFor(() => {
      expect(screen.getByText("ルート案件")).toBeInTheDocument();
    });

    fireEvent.contextMenu(screen.getByText("ルート案件"));
    fireEvent.click(await screen.findByText("＋ 子案件を作成"));
    const nameInput = await screen.findByPlaceholderText("案件名を入力");
    fireEvent.change(nameInput, { target: { value: "新しい子案件" } });
    fireEvent.click(screen.getByText("作成"));

    await waitFor(() => {
      expect(useErrorStore.getState().toasts.length).toBeGreaterThan(0);
    });
    // 失敗時はフォームを閉じない
    expect(screen.getByPlaceholderText("案件名を入力")).toBeInTheDocument();
  });
});
