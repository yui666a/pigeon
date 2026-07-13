import type { Account } from "../../types/account";
import { TrashIcon } from "../common/icons/TrashIcon";

interface AccountListProps {
  accounts: Account[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onRemove: (id: string) => void;
  onReauth?: (id: string) => void;
  onBackfill?: (id: string) => void;
  /** 現在バックフィル実行中のアカウントID（多重クリック防止のボタン無効化用） */
  backfillingAccountId?: string | null;
  /** account_id -> これ以上サーバーに古いメールが無いか */
  backfillExhausted?: Record<string, boolean>;
}

export function AccountList({
  accounts,
  selectedId,
  onSelect,
  onRemove,
  onReauth,
  onBackfill,
  backfillingAccountId,
  backfillExhausted,
}: AccountListProps) {
  if (accounts.length === 0) {
    return <p className="px-4 py-2 text-sm text-gray-400">アカウントなし</p>;
  }
  return (
    <ul className="flex flex-col">
      {accounts.map((account) => (
        <li key={account.id}>
          <div
            className={`flex items-center px-4 py-2 hover:bg-gray-100 ${selectedId === account.id ? "bg-blue-50" : ""}`}
          >
            <button
              onClick={() => onSelect(account.id)}
              className={`flex-1 text-left text-sm ${selectedId === account.id ? "font-semibold text-blue-700" : ""}`}
            >
              <div className="flex items-center gap-1.5">
                {account.provider === "google" && (
                  <span
                    className="text-xs font-bold text-blue-600"
                    title="Google"
                  >
                    G
                  </span>
                )}
                <span>{account.name}</span>
                {account.needs_reauth && (
                  <span
                    className="text-xs text-amber-500"
                    title="再認証が必要です"
                  >
                    !
                  </span>
                )}
              </div>
              <div className="text-xs text-gray-400">{account.email}</div>
            </button>
            {account.needs_reauth && onReauth && (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  onReauth(account.id);
                }}
                className="ml-1 shrink-0 rounded px-2 py-1 text-xs text-amber-600 hover:bg-amber-50"
                title="再認証"
              >
                再認証
              </button>
            )}
            {!account.needs_reauth && onBackfill && (() => {
              const isBackfilling = backfillingAccountId === account.id;
              const isExhausted = backfillExhausted?.[account.id] === true;
              return (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    onBackfill(account.id);
                  }}
                  disabled={isBackfilling || isExhausted}
                  className="ml-1 shrink-0 rounded px-2 py-1 text-xs text-gray-500 hover:bg-gray-100 disabled:cursor-not-allowed disabled:opacity-50"
                  title="過去のメールを取得"
                >
                  {isBackfilling ? "取得中…" : isExhausted ? "全件取得済み" : "過去のメールを取得"}
                </button>
              );
            })()}
            <button
              onClick={(e) => {
                e.stopPropagation();
                onRemove(account.id);
              }}
              className="ml-1 shrink-0 rounded p-1 text-gray-300 hover:bg-red-50 hover:text-red-500"
              title="アカウントを削除"
            >
              <TrashIcon className="h-3.5 w-3.5" />
            </button>
          </div>
        </li>
      ))}
    </ul>
  );
}
