import type { Account } from "../../types/account";

interface AccountListProps {
  accounts: Account[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}

export function AccountList({
  accounts,
  selectedId,
  onSelect,
}: AccountListProps) {
  if (accounts.length === 0) {
    return <p className="px-4 py-2 text-sm text-gray-400">アカウントなし</p>;
  }
  return (
    <ul className="flex flex-col">
      {accounts.map((account) => (
        <li key={account.id}>
          <button
            onClick={() => onSelect(account.id)}
            className={`w-full px-4 py-2 text-left text-sm hover:bg-gray-100 ${selectedId === account.id ? "bg-blue-50 font-semibold text-blue-700" : ""}`}
          >
            <div>{account.name}</div>
            <div className="text-xs text-gray-400">{account.email}</div>
          </button>
        </li>
      ))}
    </ul>
  );
}
