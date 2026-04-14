import { useEffect, useState } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useProjectStore } from "../../stores/projectStore";
import { useUiStore } from "../../stores/uiStore";
import { AccountList } from "./AccountList";
import { AccountForm } from "./AccountForm";
import { ProjectTree } from "./ProjectTree";
import { ProjectForm } from "./ProjectForm";
import type { CreateAccountRequest } from "../../types/account";

export function Sidebar() {
  const {
    accounts,
    selectedAccountId,
    fetchAccounts,
    createAccount,
    removeAccount,
    selectAccount,
    initDeepLinkListener,
  } = useAccountStore();
  const { createProject } = useProjectStore();
  const setViewMode = useUiStore((s) => s.setViewMode);
  const [showForm, setShowForm] = useState(false);
  const [showProjectForm, setShowProjectForm] = useState(false);

  useEffect(() => {
    fetchAccounts();
  }, [fetchAccounts]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    initDeepLinkListener().then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [initDeepLinkListener]);

  const handleSubmit = async (req: CreateAccountRequest) => {
    await createAccount(req);
    setShowForm(false);
  };

  const handleSelectAccount = (id: string) => {
    selectAccount(id);
    setViewMode("threads");
  };

  const handleProjectSubmit = async (
    name: string,
    description?: string,
    color?: string,
  ) => {
    if (!selectedAccountId) return;
    await createProject(selectedAccountId, name, description, color);
    setShowProjectForm(false);
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
          onSelect={handleSelectAccount}
          onRemove={removeAccount}
        />
        <ProjectTree
          onSelectUnclassified={() => setViewMode("unclassified")}
          onSelectProject={() => setViewMode("project")}
        />
      </div>
      {selectedAccountId && (
        <div className="border-t">
          {showProjectForm ? (
            <ProjectForm
              onSubmit={handleProjectSubmit}
              onCancel={() => setShowProjectForm(false)}
            />
          ) : (
            <button
              onClick={() => setShowProjectForm(true)}
              className="w-full px-4 py-3 text-left text-sm text-blue-600 hover:bg-gray-100 hover:underline"
            >
              + 案件を作成
            </button>
          )}
        </div>
      )}
    </aside>
  );
}
