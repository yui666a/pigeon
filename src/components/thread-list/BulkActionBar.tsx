import type { Project } from "../../types/project";

interface BulkActionBarProps {
  selectedCount: number;
  projects: Project[];
  onDelete: () => void;
  onArchive: () => void;
  onMove: (projectId: string) => void;
  onClear: () => void;
  onCreateProject: () => void;
}

/**
 * 選択中スレッドがある場合に一覧上部へ表示する一括操作バー。
 * 削除・アーカイブ・案件へ移動・選択解除を提供する
 * （設計書 2026-07-13-bulk-actions-design.md）
 */
export function BulkActionBar({
  selectedCount,
  projects,
  onDelete,
  onArchive,
  onMove,
  onClear,
  onCreateProject,
}: BulkActionBarProps) {
  if (selectedCount === 0) return null;

  return (
    <div className="flex flex-wrap items-center gap-2 border-b bg-blue-50 px-4 py-2">
      <span className="shrink-0 whitespace-nowrap text-sm text-blue-800">
        {selectedCount} 件選択中
      </span>
      <div className="flex flex-1 flex-wrap items-center justify-end gap-2">
        <select
          aria-label="案件へ移動"
          defaultValue=""
          onChange={(e) => {
            if (e.target.value) {
              onMove(e.target.value);
              e.target.value = "";
            }
          }}
          className="min-w-0 flex-1 rounded border px-2 py-1 text-sm"
        >
          <option value="" disabled>
            案件へ移動...
          </option>
          {projects.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
        <button
          onClick={onCreateProject}
          className="shrink-0 whitespace-nowrap rounded border border-blue-300 px-3 py-1 text-sm text-blue-700 hover:bg-blue-100"
        >
          ＋ 新しい案件
        </button>
        <button
          onClick={onArchive}
          className="shrink-0 whitespace-nowrap rounded border px-3 py-1 text-sm hover:bg-blue-100"
        >
          アーカイブ
        </button>
        <button
          onClick={onDelete}
          className="shrink-0 whitespace-nowrap rounded border px-3 py-1 text-sm text-red-600 hover:bg-red-50"
        >
          削除
        </button>
        <button
          onClick={onClear}
          className="shrink-0 whitespace-nowrap rounded border px-3 py-1 text-sm hover:bg-gray-100"
        >
          選択解除
        </button>
      </div>
    </div>
  );
}
