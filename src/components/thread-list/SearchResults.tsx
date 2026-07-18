import { useEffect, useMemo, useState } from "react";
import DOMPurify from "dompurify";
import { useSearchStore } from "../../stores/searchStore";
import { useSavedSearchStore } from "../../stores/savedSearchStore";
import { useMailStore } from "../../stores/mailStore";
import { EmptyState } from "../common/EmptyState";
import type { SearchResult } from "../../types/mail";

type SortBy = "relevance" | "date";

/** Sanitize FTS5 snippet HTML, allowing only <b> tags for highlights. */
function sanitizeSnippet(html: string): string {
  return DOMPurify.sanitize(html, { ALLOWED_TAGS: ["b"] });
}

function SearchResultItem({
  result,
  selected,
  onClick,
}: {
  result: SearchResult;
  selected: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      aria-selected={selected}
      className={`w-full border-b px-4 py-3 text-left hover:bg-gray-50 ${
        selected ? "bg-blue-50" : ""
      }`}
    >
      <div className="flex items-center justify-between">
        <span className="truncate text-sm font-medium">
          {result.mail.subject}
        </span>
        <span className="ml-2 shrink-0 text-xs text-gray-400">
          {result.mail.date.slice(0, 10)}
        </span>
      </div>
      <div className="mt-1 flex items-center gap-2">
        <span className="truncate text-xs text-gray-500">
          {result.mail.from_addr}
        </span>
        <span
          className={`shrink-0 rounded px-1.5 py-0.5 text-xs ${
            result.project_name
              ? "bg-blue-100 text-blue-700"
              : "bg-gray-100 text-gray-500"
          }`}
        >
          {result.project_name ?? "未分類"}
        </span>
      </div>
      <p
        className="mt-1 truncate text-xs text-gray-400"
        dangerouslySetInnerHTML={{ __html: sanitizeSnippet(result.snippet) }}
      />
    </button>
  );
}

export function SearchResults() {
  const query = useSearchStore((s) => s.query);
  const mode = useSearchStore((s) => s.mode);
  const results = useSearchStore((s) => s.results);
  const searching = useSearchStore((s) => s.searching);
  const selectedIndex = useSearchStore((s) => s.selectedIndex);
  const setSelectedIndex = useSearchStore((s) => s.setSelectedIndex);
  const createSaved = useSavedSearchStore((s) => s.create);
  const selectThread = useMailStore((s) => s.selectThread);
  const selectMail = useMailStore((s) => s.selectMail);

  const [saving, setSaving] = useState(false);
  const [saveName, setSaveName] = useState("");
  const [sortBy, setSortBy] = useState<SortBy>("relevance");

  // 表示配列。selectedIndex（j/k ナビ）と右ペイン同期の両方が
  // この単一の配列を参照することで、表示順と選択対象の不一致を防ぐ。
  // 日付順は mail.date の ISO 文字列を降順（localeCompare）で並べる。
  const displayResults = useMemo(
    () =>
      sortBy === "date"
        ? [...results].sort((a, b) => b.mail.date.localeCompare(a.mail.date))
        : results,
    [results, sortBy],
  );

  // j/k ナビによる selectedIndex の変化を右ペインに反映する
  // （クリック選択と同じ経路。selectThread(null) が先でないと
  // MailView が古い MailTabs を表示し続ける）
  useEffect(() => {
    if (selectedIndex === -1) return;
    const result = displayResults[selectedIndex];
    if (!result) return;
    selectThread(null);
    selectMail(result.mail);
  }, [selectedIndex, displayResults, selectThread, selectMail]);

  const handleResultClick = (index: number) => {
    setSelectedIndex(index);
  };

  const handleSave = () => {
    if (!saveName.trim()) return;
    void createSaved(saveName.trim(), query, mode);
    setSaving(false);
    setSaveName("");
  };

  if (searching) {
    return <EmptyState message="検索中..." />;
  }

  if (query && results.length === 0) {
    return <EmptyState message={`「${query}」の検索結果がありません`} />;
  }

  if (!query) {
    return <EmptyState message="キーワードを入力して検索" />;
  }

  return (
    <div className="h-full overflow-y-auto">
      <div className="flex flex-wrap items-center gap-2 border-b bg-gray-50 px-4 py-2 text-xs text-gray-500">
        <span>
          「{query}」の検索結果: {results.length}件
        </span>
        <div className="ml-auto flex items-center gap-1">
          <button
            type="button"
            aria-pressed={sortBy === "relevance"}
            onClick={() => setSortBy("relevance")}
            className={`rounded px-1.5 py-0.5 ${
              sortBy === "relevance"
                ? "bg-blue-100 text-blue-700"
                : "text-gray-500 hover:bg-gray-100"
            }`}
          >
            関連度順
          </button>
          <button
            type="button"
            aria-pressed={sortBy === "date"}
            onClick={() => setSortBy("date")}
            className={`rounded px-1.5 py-0.5 ${
              sortBy === "date"
                ? "bg-blue-100 text-blue-700"
                : "text-gray-500 hover:bg-gray-100"
            }`}
          >
            日付順
          </button>
          {saving ? (
            <input
              autoFocus
              placeholder="ビュー名"
              value={saveName}
              onChange={(e) => setSaveName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleSave();
                if (e.key === "Escape") {
                  setSaving(false);
                  setSaveName("");
                }
              }}
              className="w-24 rounded border px-1 py-0.5 text-xs"
            />
          ) : (
            <button
              type="button"
              onClick={() => setSaving(true)}
              className="rounded px-1.5 py-0.5 text-blue-600 hover:bg-gray-100 hover:underline"
            >
              この検索を保存
            </button>
          )}
        </div>
      </div>
      {displayResults.map((result, index) => (
        <SearchResultItem
          key={result.mail.id}
          result={result}
          selected={index === selectedIndex}
          onClick={() => handleResultClick(index)}
        />
      ))}
    </div>
  );
}
