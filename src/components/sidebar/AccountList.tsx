import type { Account } from "../../types/account";

interface AccountListProps {
  accounts: Account[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onRemove: (id: string) => void;
  onReauth?: (id: string) => void;
}

export function AccountList({
  accounts,
  selectedId,
  onSelect,
  onRemove,
  onReauth,
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
            <button
              onClick={(e) => {
                e.stopPropagation();
                onRemove(account.id);
              }}
              className="ml-1 shrink-0 rounded p-1 text-gray-300 hover:bg-red-50 hover:text-red-500"
              title="アカウントを削除"
            >
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" className="h-3.5 w-3.5">
                <path fillRule="evenodd" d="M8.75 1A2.75 2.75 0 006 3.75v.443c-.795.077-1.584.176-2.365.298a.75.75 0 10.23 1.482l.149-.022.841 10.518A2.75 2.75 0 007.596 19h4.807a2.75 2.75 0 002.742-2.53l.841-10.519.149.023a.75.75 0 00.23-1.482A41.03 41.03 0 0014 4.193V3.75A2.75 2.75 0 0011.25 1h-2.5zM10 4c.84 0 1.673.025 2.5.075V3.75c0-.69-.56-1.25-1.25-1.25h-2.5c-.69 0-1.25.56-1.25 1.25v.325C8.327 4.025 9.16 4 10 4zM8.58 7.72a.75.75 0 00-1.5.06l.3 7.5a.75.75 0 101.5-.06l-.3-7.5zm4.34.06a.75.75 0 10-1.5-.06l-.3 7.5a.75.75 0 101.5.06l.3-7.5z" clipRule="evenodd" />
              </svg>
            </button>
          </div>
        </li>
      ))}
    </ul>
  );
}
