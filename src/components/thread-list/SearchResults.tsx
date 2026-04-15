import DOMPurify from "dompurify";
import { useSearchStore } from "../../stores/searchStore";
import { useMailStore } from "../../stores/mailStore";
import { EmptyState } from "../common/EmptyState";
import type { SearchResult } from "../../types/mail";

/** Sanitize FTS5 snippet HTML, allowing only <b> tags for highlights. */
function sanitizeSnippet(html: string): string {
  return DOMPurify.sanitize(html, { ALLOWED_TAGS: ["b"] });
}

function SearchResultItem({
  result,
  onClick,
}: {
  result: SearchResult;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="w-full border-b px-4 py-3 text-left hover:bg-gray-50"
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
  const results = useSearchStore((s) => s.results);
  const searching = useSearchStore((s) => s.searching);
  const selectThread = useMailStore((s) => s.selectThread);
  const selectMail = useMailStore((s) => s.selectMail);

  const handleResultClick = (result: SearchResult) => {
    // Clear any existing thread selection first to prevent MailView
    // from rendering stale MailTabs
    selectThread(null);
    selectMail(result.mail);
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
      <div className="border-b bg-gray-50 px-4 py-2 text-xs text-gray-500">
        「{query}」の検索結果: {results.length}件
      </div>
      {results.map((result) => (
        <SearchResultItem
          key={result.mail.id}
          result={result}
          onClick={() => handleResultClick(result)}
        />
      ))}
    </div>
  );
}
