import { useEffect, useState } from "react";
import { ContextMenu } from "../common/ContextMenu";
import { useSavedSearchStore } from "../../stores/savedSearchStore";
import { useSearchStore } from "../../stores/searchStore";
import { useUiStore } from "../../stores/uiStore";
import { useProjectStore } from "../../stores/projectStore";

interface SmartViewListProps {
  accountId: string | null;
}

/**
 * サイドバーの「スマートビュー」（保存検索）セクション。
 * 行クリックで保存されたモード・クエリで検索を再実行し検索ビューへ切り替える。
 * 右クリックで名前変更（インライン入力）・削除ができる。0件時は非表示。
 */
export function SmartViewList({ accountId }: SmartViewListProps) {
  const savedSearches = useSavedSearchStore((s) => s.savedSearches);
  const fetch = useSavedSearchStore((s) => s.fetch);
  const rename = useSavedSearchStore((s) => s.rename);
  const remove = useSavedSearchStore((s) => s.remove);
  const setMode = useSearchStore((s) => s.setMode);
  const search = useSearchStore((s) => s.search);
  const setViewMode = useUiStore((s) => s.setViewMode);
  const selectedProjectId = useProjectStore((s) => s.selectedProjectId);
  const [menu, setMenu] = useState<{ x: number; y: number; id: number } | null>(
    null,
  );
  const [renaming, setRenaming] = useState<{ id: number; value: string } | null>(
    null,
  );

  useEffect(() => {
    void fetch();
  }, [fetch]);

  if (savedSearches.length === 0) return null;

  const run = (id: number) => {
    const s = savedSearches.find((v) => v.id === id);
    if (!s || !accountId) return;
    setMode(s.mode);
    void search(accountId, s.query, selectedProjectId ?? undefined);
    setViewMode("search");
  };

  return (
    <div className="mt-2">
      <div className="px-4 py-1">
        <span className="text-xs font-semibold uppercase tracking-wide text-gray-400">
          スマートビュー
        </span>
      </div>
      <ul className="flex flex-col">
        {savedSearches.map((s) => (
          <li key={s.id}>
            {renaming?.id === s.id ? (
              <input
                autoFocus
                className="mx-4 my-1 w-11/12 rounded border px-1 text-sm"
                value={renaming.value}
                onChange={(e) => setRenaming({ id: s.id, value: e.target.value })}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && renaming.value.trim()) {
                    void rename(s.id, renaming.value.trim());
                    setRenaming(null);
                  }
                  if (e.key === "Escape") setRenaming(null);
                }}
                onBlur={() => setRenaming(null)}
              />
            ) : (
              <button
                type="button"
                className="w-full px-4 py-2 text-left text-sm hover:bg-gray-100"
                onClick={() => run(s.id)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenu({ x: e.clientX, y: e.clientY, id: s.id });
                }}
              >
                <span className="flex items-center gap-2">
                  <span className="text-gray-400">🔎</span>
                  <span className="truncate">{s.name}</span>
                </span>
              </button>
            )}
          </li>
        ))}
      </ul>
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          items={[
            {
              label: "名前変更",
              onClick: () => {
                const s = savedSearches.find((v) => v.id === menu.id);
                if (s) setRenaming({ id: s.id, value: s.name });
              },
            },
            { label: "削除", danger: true, onClick: () => void remove(menu.id) },
          ]}
        />
      )}
    </div>
  );
}
