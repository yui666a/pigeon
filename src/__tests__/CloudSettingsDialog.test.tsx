import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { CloudSettingsDialog } from "../components/sidebar/CloudSettingsDialog";
import type { Project } from "../types/project";
import type { ProjectDirectory } from "../types/directory";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

const project: Project = {
  id: "p1", account_id: "acc1", name: "春公演", description: null,
  color: null, is_archived: false, created_at: "", updated_at: "",
};
const directory: ProjectDirectory = {
  id: "d1", project_id: "p1", path: "/tmp/stage-a", is_primary: true,
  status: "ok", last_scanned_at: null, created_at: "",
};

const files = [
  { id: "f1", directory_id: "d1", relative_path: "図面/平面図.pdf", size_bytes: 100, mtime: "", content_hash: null, content_kind: "pdf", extract_status: "unsupported", indexed_at: "" },
  { id: "f2", directory_id: "d1", relative_path: "香盤表.md", size_bytes: 50, mtime: "", content_hash: "h", content_kind: "text", extract_status: "ok", indexed_at: "" },
];

function setupInvoke(rules: unknown[] = [], context: unknown = null) {
  mockInvoke.mockImplementation((cmd: unknown) => {
    switch (cmd) {
      case "list_project_files": return Promise.resolve(files);
      case "get_cloud_rules": return Promise.resolve(rules);
      case "get_project_context": return Promise.resolve(context);
      default: return Promise.resolve(null);
    }
  });
}

describe("CloudSettingsDialog", () => {
  beforeEach(() => vi.clearAllMocks());

  it("renders file tree with all checkboxes off by default (deny by default)", async () => {
    setupInvoke();
    render(<CloudSettingsDialog project={project} directory={directory} onClose={vi.fn()} />);

    await screen.findByText(/香盤表\.md/); // ノードは「📄 香盤表.md」なので部分一致
    expect(screen.getByText(/平面図\.pdf/)).toBeInTheDocument();
    const checkboxes = screen.getAllByRole("checkbox");
    // 案件単位トグル + フォルダ「図面」 + ファイル2件
    for (const cb of checkboxes) {
      expect(cb).not.toBeChecked();
    }
  });

  it("checking a file sets an explicit allow rule", async () => {
    setupInvoke();
    render(<CloudSettingsDialog project={project} directory={directory} onClose={vi.fn()} />);
    const fileRow = await screen.findByText(/香盤表\.md/);
    const checkbox = fileRow.closest("li")!.querySelector("input[type=checkbox]")!;

    fireEvent.click(checkbox);

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("set_cloud_rule", {
        directoryId: "d1",
        scope: "file",
        relativePath: "香盤表.md",
        allow: true,
      });
    });
  });

  it("shows checked state derived from existing rules (directory rule cascades)", async () => {
    setupInvoke([
      { id: "r1", directory_id: "d1", scope: "directory", relative_path: "図面", allow: true },
    ]);
    render(<CloudSettingsDialog project={project} directory={directory} onClose={vi.fn()} />);
    const pdfRow = await screen.findByText(/平面図\.pdf/);
    const checkbox = pdfRow.closest("li")!.querySelector("input[type=checkbox]")!;
    expect(checkbox).toBeChecked();
  });

  it("toggles allow_cloud_context and shows context preview", async () => {
    setupInvoke([], {
      project_id: "p1", cached_context: "会場: 〇〇ホール", context_hash: null,
      inventory_hash: null, allow_cloud_context: false, generated_at: null,
    });
    render(<CloudSettingsDialog project={project} directory={directory} onClose={vi.fn()} />);

    expect(await screen.findByText(/会場: 〇〇ホール/)).toBeInTheDocument();
    const toggle = screen.getByLabelText(/コンテキストファイルをクラウドLLMへ送信/);
    fireEvent.click(toggle);
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("set_allow_cloud_context", {
        projectId: "p1",
        allow: true,
      });
    });
  });

  it("shows the local-LLM notice", async () => {
    setupInvoke();
    render(<CloudSettingsDialog project={project} directory={directory} onClose={vi.fn()} />);
    expect(await screen.findByText(/ローカルLLM/)).toBeInTheDocument();
  });

  it("re-syncs rules from the backend even when set_cloud_rule fails (display must never diverge from actual rules)", async () => {
    // 初回ロードは成功、トグル時の set_cloud_rule は失敗させる。
    // get_cloud_rules は毎回成功させ、失敗後も再取得されることを検証する。
    let getCloudRulesCallCount = 0;
    mockInvoke.mockImplementation((cmd: unknown) => {
      switch (cmd) {
        case "list_project_files":
          return Promise.resolve(files);
        case "get_cloud_rules":
          getCloudRulesCallCount += 1;
          return Promise.resolve([]);
        case "get_project_context":
          return Promise.resolve(null);
        case "set_cloud_rule":
          return Promise.reject(new Error("backend rejected"));
        default:
          return Promise.resolve(null);
      }
    });

    render(<CloudSettingsDialog project={project} directory={directory} onClose={vi.fn()} />);
    const fileRow = await screen.findByText(/香盤表\.md/);
    const checkbox = fileRow.closest("li")!.querySelector("input[type=checkbox]")!;

    await waitFor(() => expect(getCloudRulesCallCount).toBe(1)); // 初期ロード分

    fireEvent.click(checkbox);

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("set_cloud_rule", expect.anything());
    });

    // 失敗しても必ず get_cloud_rules が再度呼ばれ、表示が実ルールと再同期されること
    await waitFor(() => {
      expect(getCloudRulesCallCount).toBe(2);
    });
  });

  it("applies exactly two ops (delete stray directory rule, then set file rule) when unchecking a file with a leftover directory-scope rule on the same path", async () => {
    // 「香盤表.md」に本来ありえないディレクトリスコープの残留ルール(allow:true)がある状態。
    // ルート("")の directory ルールが allow:true なので、この残留ルールを削除しただけでは
    // 継承値が true のままとなり、チェックを外す(desired=false)には file スコープの
    // set(allow:false) も追加で必要になる ―― という2 ops ケースを再現する
    // (planToggle の実装上、残留ルールのみでは 1 op にしかならないため、
    //  継承元となるルート directory ルールを別途用意している)。
    setupInvoke([
      { id: "r0", directory_id: "d1", scope: "directory", relative_path: "", allow: true },
      { id: "r1", directory_id: "d1", scope: "directory", relative_path: "香盤表.md", allow: true },
    ]);
    render(<CloudSettingsDialog project={project} directory={directory} onClose={vi.fn()} />);
    const fileRow = await screen.findByText(/香盤表\.md/);
    const checkbox = fileRow.closest("li")!.querySelector("input[type=checkbox]")!;
    expect(checkbox).toBeChecked();

    fireEvent.click(checkbox);

    await waitFor(() => {
      const setCloudRuleCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "set_cloud_rule");
      expect(setCloudRuleCalls).toHaveLength(2);
    });

    const setCloudRuleCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "set_cloud_rule");
    expect(setCloudRuleCalls[0][1]).toEqual({
      directoryId: "d1",
      scope: "directory",
      relativePath: "香盤表.md",
      allow: null,
    });
    expect(setCloudRuleCalls[1][1]).toEqual({
      directoryId: "d1",
      scope: "file",
      relativePath: "香盤表.md",
      allow: false,
    });
  });

  it("buildTree distinguishes a file and a directory sharing the same name", async () => {
    const filesWithNameCollision = [
      { id: "f1", directory_id: "d1", relative_path: "資料", size_bytes: 10, mtime: "", content_hash: "h", content_kind: "text", extract_status: "ok", indexed_at: "" },
      { id: "f2", directory_id: "d1", relative_path: "資料/内訳.txt", size_bytes: 20, mtime: "", content_hash: "h", content_kind: "text", extract_status: "ok", indexed_at: "" },
    ];
    mockInvoke.mockImplementation((cmd: unknown) => {
      switch (cmd) {
        case "list_project_files":
          return Promise.resolve(filesWithNameCollision);
        case "get_cloud_rules":
          return Promise.resolve([]);
        case "get_project_context":
          return Promise.resolve(null);
        default:
          return Promise.resolve(null);
      }
    });

    render(<CloudSettingsDialog project={project} directory={directory} onClose={vi.fn()} />);

    expect(await screen.findByText(/📄 資料$/)).toBeInTheDocument();
    expect(await screen.findByText(/📂 資料\//)).toBeInTheDocument();
    expect(await screen.findByText(/内訳\.txt/)).toBeInTheDocument();
  });
});
