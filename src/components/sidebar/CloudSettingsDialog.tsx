import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Project } from "../../types/project";
import type {
  CloudRule,
  ProjectContext,
  ProjectDirectory,
  ProjectFile,
} from "../../types/directory";
import { effectiveAllow, planToggle } from "../../utils/cloudPolicy";
import { useErrorStore } from "../../stores/errorStore";

interface CloudSettingsDialogProps {
  project: Project;
  directory: ProjectDirectory;
  onClose: () => void;
}

interface TreeNode {
  name: string;
  path: string; // ディレクトリからの相対パス
  isDir: boolean;
  children: TreeNode[];
}

/** relative_path のリストからツリーを構築する（ディレクトリ優先・名前順） */
function buildTree(files: ProjectFile[]): TreeNode[] {
  const root: TreeNode = { name: "", path: "", isDir: true, children: [] };
  for (const file of files) {
    const segments = file.relative_path.split("/");
    let node = root;
    let pathSoFar = "";
    segments.forEach((segment, i) => {
      pathSoFar = pathSoFar ? `${pathSoFar}/${segment}` : segment;
      const isDir = i < segments.length - 1;
      let child = node.children.find((c) => c.name === segment && c.isDir === isDir);
      if (!child) {
        child = { name: segment, path: pathSoFar, isDir, children: [] };
        node.children.push(child);
      }
      node = child;
    });
  }
  const sortRec = (n: TreeNode) => {
    n.children.sort((a, b) =>
      a.isDir === b.isDir ? a.name.localeCompare(b.name, "ja") : a.isDir ? -1 : 1,
    );
    n.children.forEach(sortRec);
  };
  sortRec(root);
  return root.children;
}

export function CloudSettingsDialog({
  project,
  directory,
  onClose,
}: CloudSettingsDialogProps) {
  const [files, setFiles] = useState<ProjectFile[]>([]);
  const [rules, setRules] = useState<CloudRule[]>([]);
  const [context, setContext] = useState<ProjectContext | null>(null);
  const [loading, setLoading] = useState(true);

  const reload = useCallback(async () => {
    try {
      const [filesRes, rulesRes, contextRes] = await Promise.all([
        invoke<ProjectFile[]>("list_project_files", { directoryId: directory.id }),
        invoke<CloudRule[]>("get_cloud_rules", { directoryId: directory.id }),
        invoke<ProjectContext | null>("get_project_context", { projectId: project.id }),
      ]);
      setFiles(filesRes);
      setRules(rulesRes);
      setContext(contextRes);
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    } finally {
      setLoading(false);
    }
  }, [directory.id, project.id]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const tree = useMemo(() => buildTree(files), [files]);

  const handleToggleNode = async (node: TreeNode) => {
    const scope = node.isDir ? "directory" : "file";
    const ops = planToggle(rules, scope, node.path);
    try {
      for (const op of ops) {
        await invoke("set_cloud_rule", {
          directoryId: directory.id,
          scope: op.scope,
          relativePath: node.path,
          allow: op.action === "set" ? op.allow : null,
        });
      }
      const rulesRes = await invoke<CloudRule[]>("get_cloud_rules", {
        directoryId: directory.id,
      });
      setRules(rulesRes);
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  };

  const handleToggleContext = async () => {
    const allow = !(context?.allow_cloud_context ?? false);
    try {
      await invoke("set_allow_cloud_context", { projectId: project.id, allow });
      const contextRes = await invoke<ProjectContext | null>("get_project_context", {
        projectId: project.id,
      });
      setContext(contextRes);
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  };

  const renderNode = (node: TreeNode, depth: number): React.ReactNode => (
    <li key={`${node.isDir ? "d" : "f"}:${node.path}`}>
      <div
        className="flex items-center gap-2 py-1"
        style={{ paddingLeft: `${depth * 20}px` }}
      >
        <input
          type="checkbox"
          checked={effectiveAllow(rules, node.path)}
          onChange={() => void handleToggleNode(node)}
          className="h-4 w-4"
        />
        <span className="text-sm">
          {node.isDir ? "📂" : "📄"} {node.name}
          {node.isDir && "/"}
        </span>
      </div>
      {node.children.length > 0 && (
        <ul>{node.children.map((c) => renderNode(c, depth + 1))}</ul>
      )}
    </li>
  );

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="flex max-h-[80vh] w-[560px] flex-col rounded-lg bg-white shadow-xl">
        <div className="border-b px-5 py-3">
          <h2 className="text-sm font-bold">クラウド送信設定 — {project.name}</h2>
          <p className="mt-0.5 text-xs text-gray-500">
            チェックしたものだけがクラウドLLMへの入力に使われます（デフォルトはすべて送信オフ）。
          </p>
        </div>
        <div className="flex-1 overflow-y-auto px-5 py-3">
          <p className="mb-3 rounded bg-blue-50 px-3 py-2 text-xs text-blue-700">
            現在ローカルLLM（Ollama）使用中のため、データは外部に送信されません。
            この設定は保存され、クラウドLLM導入時に適用されます。
          </p>

          <label className="mb-1 flex items-start gap-2 rounded border border-gray-200 bg-gray-50 px-3 py-2">
            <input
              type="checkbox"
              checked={context?.allow_cloud_context ?? false}
              onChange={() => void handleToggleContext()}
              className="mt-0.5 h-4 w-4"
              aria-label="コンテキストファイルをクラウドLLMへ送信する"
            />
            <span className="text-sm">
              コンテキストファイル（PIGEON-CONTEXT.md）をクラウドLLMへ送信する
              <span className="block text-xs text-gray-500">
                分類のたびに以下の内容がプロンプトへ入ります。内容を確認してからONにしてください。
              </span>
            </span>
          </label>
          <pre className="mb-4 max-h-32 overflow-y-auto whitespace-pre-wrap rounded border border-gray-200 bg-gray-50 px-3 py-2 text-xs text-gray-600">
            {context?.cached_context ?? "（コンテキスト未生成。再スキャンで生成されます）"}
          </pre>

          <div className="mb-1 text-xs font-semibold uppercase tracking-wide text-gray-400">
            ファイルごとの送信許可
          </div>
          {loading ? (
            <p className="py-4 text-center text-sm text-gray-400">読み込み中…</p>
          ) : files.length === 0 ? (
            <p className="py-4 text-center text-sm text-gray-400">
              ファイルがありません。再スキャンしてください。
            </p>
          ) : (
            <ul>{tree.map((n) => renderNode(n, 0))}</ul>
          )}
        </div>
        <div className="flex justify-end border-t px-5 py-3">
          <button
            onClick={onClose}
            className="rounded bg-blue-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-blue-700"
          >
            閉じる
          </button>
        </div>
      </div>
    </div>
  );
}
