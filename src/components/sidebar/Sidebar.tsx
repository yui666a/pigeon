import { useEffect, useState } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { AccountList } from "./AccountList";
import { AccountForm } from "./AccountForm";
import type { CreateAccountRequest } from "../../types/account";

export function Sidebar() {
  const {
    accounts,
    selectedAccountId,
    fetchAccounts,
    createAccount,
    selectAccount,
  } = useAccountStore();
  const [showForm, setShowForm] = useState(false);

  useEffect(() => {
    fetchAccounts();
  }, [fetchAccounts]);

  const handleSubmit = async (req: CreateAccountRequest) => {
    await createAccount(req);
    setShowForm(false);
  };

  return (
    <aside className="flex h-full w-64 flex-col border-r bg-gray-50">
      <div className="flex items-center justify-between border-b px-4 py-3">
        <h1 className="text-lg font-bold">Pigeon</h1>
        <button
          onClick={() => setShowForm(!showForm)}
          className="text-sm text-blue-600 hover:underline"
        >
          {showForm ? "閉じる" : "+ 追加"}
        </button>
      </div>
      {showForm && (
        <AccountForm
          onSubmit={handleSubmit}
          onCancel={() => setShowForm(false)}
        />
      )}
      <div className="flex-1 overflow-y-auto">
        <AccountList
          accounts={accounts}
          selectedId={selectedAccountId}
          onSelect={selectAccount}
        />
      </div>
    </aside>
  );
}
