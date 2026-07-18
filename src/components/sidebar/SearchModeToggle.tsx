import { useSearchStore } from "../../stores/searchStore";
import type { SearchMode } from "../../types/search";

interface SearchModeToggleProps {
  accountId?: string | null;
  selectedProjectId?: string | null;
}

const MODES: { value: SearchMode; label: string }[] = [
  { value: "fulltext", label: "文字列" },
  { value: "semantic", label: "ベクトル" },
];

export function SearchModeToggle({ accountId, selectedProjectId }: SearchModeToggleProps) {
  const mode = useSearchStore((s) => s.mode);
  const setMode = useSearchStore((s) => s.setMode);
  const query = useSearchStore((s) => s.query);
  const search = useSearchStore((s) => s.search);

  const handleSelect = (next: SearchMode) => {
    if (next === mode) return;
    setMode(next);
    // アクティブな検索があれば新モードで即再実行（結果とモードの不整合を残さない）。
    // 案件内検索スコープが有効な場合は再検索でもスコープを維持する
    if (query && accountId) {
      void search(accountId, query, selectedProjectId ?? null);
    }
  };

  return (
    <div className="flex gap-1 px-3 pb-1">
      {MODES.map((m) => (
        <button
          key={m.value}
          type="button"
          aria-pressed={mode === m.value}
          onClick={() => handleSelect(m.value)}
          className={`rounded px-2 py-0.5 text-xs ${
            mode === m.value
              ? "bg-blue-100 text-blue-700 font-semibold"
              : "text-gray-500 hover:bg-gray-100"
          }`}
        >
          {m.label}
        </button>
      ))}
    </div>
  );
}
