import { useSearchStore } from "../../stores/searchStore";

interface SearchScopeToggleProps {
  selectedProjectId: string | null;
}

/**
 * 「この案件内で検索」トグル。案件選択中のみ表示する（デフォルトOFF）。
 * 状態は searchStore に持たせ、選択案件が変わっても維持する（OFF に戻さない）。
 */
export function SearchScopeToggle({ selectedProjectId }: SearchScopeToggleProps) {
  const scopeToProject = useSearchStore((s) => s.scopeToProject);
  const setScopeToProject = useSearchStore((s) => s.setScopeToProject);

  if (!selectedProjectId) return null;

  return (
    <label className="flex items-center gap-2 px-3 pb-1 text-xs text-gray-600">
      <input
        type="checkbox"
        aria-label="この案件内で検索"
        checked={scopeToProject}
        onChange={(e) => setScopeToProject(e.target.checked)}
        className="h-3.5 w-3.5"
      />
      この案件内で検索
    </label>
  );
}
