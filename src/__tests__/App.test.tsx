import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import App from "../App";
import { useUiStore } from "../stores/uiStore";
import { useProjectStore } from "../stores/projectStore";

// 中央ペインの組み込み条件だけを検証したいので、重い子コンポーネント群は
// スタブ化する（Tauri invoke やエディタ初期化などを持ち込まないため）。
vi.mock("../components/sidebar/Sidebar", () => ({
  Sidebar: () => <div data-testid="sidebar" />,
}));
vi.mock("../components/thread-list/ThreadList", () => ({
  ThreadList: () => <div data-testid="thread-list" />,
}));
vi.mock("../components/thread-list/UnclassifiedList", () => ({
  UnclassifiedList: () => <div data-testid="unclassified-list" />,
}));
vi.mock("../components/thread-list/SearchResults", () => ({
  SearchResults: () => <div data-testid="search-results" />,
}));
vi.mock("../components/thread-list/DraftList", () => ({
  DraftList: () => <div data-testid="draft-list" />,
}));
vi.mock("../components/mail-view/MailView", () => ({
  MailView: () => <div data-testid="mail-view" />,
}));
vi.mock("../components/common/DragOverlay", () => ({
  DragOverlay: () => <div data-testid="drag-overlay" />,
}));
vi.mock("../components/common/ToastContainer", () => ({
  ToastContainer: () => <div data-testid="toast-container" />,
}));
vi.mock("../components/compose/ComposeModal", () => ({
  ComposeModal: () => <div data-testid="compose-modal" />,
}));
vi.mock("../components/project-note/ProjectNotePanel", () => ({
  ProjectNotePanel: ({ projectId }: { projectId: string }) => (
    <div data-testid="project-note-panel">{projectId}</div>
  ),
}));
vi.mock("../hooks/useKeyboardShortcuts", () => ({
  useKeyboardShortcuts: () => {},
}));
vi.mock("../utils/notifyNewMail", () => ({
  ensureNotificationPermission: vi.fn().mockResolvedValue(true),
}));

const mockInvoke = vi.fn().mockResolvedValue([]);
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

describe("App", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useUiStore.setState({ viewMode: "threads" });
    useProjectStore.setState({ selectedProjectId: null });
  });

  it("案件が選択されていなければ案件ノートを表示しない", () => {
    useUiStore.setState({ viewMode: "project" });
    useProjectStore.setState({ selectedProjectId: null });

    render(<App />);

    expect(screen.queryByTestId("project-note-panel")).not.toBeInTheDocument();
  });

  it("案件表示モードで案件が選択されていれば案件ノートを表示する", () => {
    useUiStore.setState({ viewMode: "project" });
    useProjectStore.setState({ selectedProjectId: "p1" });

    render(<App />);

    expect(screen.getByTestId("project-note-panel")).toHaveTextContent("p1");
  });

  it("スレッドが0件の案件でも案件ノートは表示される（ThreadListのEmptyStateに埋もれない）", () => {
    // ThreadList はスタブ化しているため常に描画されるが、実コンポーネントは
    // threads.length === 0 で早期returnしてしまう。ProjectNotePanel が
    // ThreadList の外（App側）に組み込まれていることをこのテストで担保する。
    useUiStore.setState({ viewMode: "project" });
    useProjectStore.setState({ selectedProjectId: "empty-project" });

    render(<App />);

    expect(screen.getByTestId("project-note-panel")).toHaveTextContent(
      "empty-project",
    );
    expect(screen.getByTestId("thread-list")).toBeInTheDocument();
  });

  it("スレッド一覧モードでは案件ノートを表示しない", () => {
    useUiStore.setState({ viewMode: "threads" });
    useProjectStore.setState({ selectedProjectId: "p1" });

    render(<App />);

    expect(screen.queryByTestId("project-note-panel")).not.toBeInTheDocument();
  });

  it("検索モードでは案件ノートを表示しない", () => {
    useUiStore.setState({ viewMode: "search" });
    useProjectStore.setState({ selectedProjectId: "p1" });

    render(<App />);

    expect(screen.queryByTestId("project-note-panel")).not.toBeInTheDocument();
  });
});
