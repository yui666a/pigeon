import { useEffect, useState } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useComposeStore } from "../../stores/composeStore";
import { useMailStore } from "../../stores/mailStore";
import { useProjectStore } from "../../stores/projectStore";
import { useSearchStore } from "../../stores/searchStore";
import { useUiStore } from "../../stores/uiStore";
import { AccountList } from "./AccountList";

// バックフィルの既定取得件数。settings.initial_sync_limit（バックエンド既定値）と
// 同じ値をそのまま使う。設定UIは作らない（設計書 2026-07-13-mail-backfill-design.md）
const BACKFILL_LIMIT = 5000;
import { AccountForm } from "./AccountForm";
import { SearchBar } from "./SearchBar";
import { SearchModeToggle } from "./SearchModeToggle";
import { ProjectTree } from "./ProjectTree";
import { SmartViewList } from "./SmartViewList";
import { ProjectForm } from "./ProjectForm";
import { ScanIndicator } from "./ScanIndicator";
import { SyncIndicator } from "./SyncIndicator";
import { LlmSettingsDialog } from "./LlmSettingsDialog";
import { NotificationToggle } from "./NotificationToggle";
import type { CreateAccountRequest } from "../../types/account";

export function Sidebar() {
  const accounts = useAccountStore((s) => s.accounts);
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const fetchAccounts = useAccountStore((s) => s.fetchAccounts);
  const createAccount = useAccountStore((s) => s.createAccount);
  const removeAccount = useAccountStore((s) => s.removeAccount);
  const selectAccount = useAccountStore((s) => s.selectAccount);
  const startReauth = useAccountStore((s) => s.startReauth);
  const initDeepLinkListener = useAccountStore((s) => s.initDeepLinkListener);
  const createProject = useProjectStore((s) => s.createProject);
  const linkDirectory = useProjectStore((s) => s.linkDirectory);
  const rescanProject = useProjectStore((s) => s.rescanProject);
  const backfillAccount = useMailStore((s) => s.backfillAccount);
  const backfillExhausted = useMailStore((s) => s.backfillExhausted);
  const openCompose = useComposeStore((s) => s.openCompose);
  const search = useSearchStore((s) => s.search);
  const clearSearch = useSearchStore((s) => s.clearSearch);
  const setViewMode = useUiStore((s) => s.setViewMode);
  const [showForm, setShowForm] = useState(false);
  const [showProjectForm, setShowProjectForm] = useState(false);
  const [showLlmSettings, setShowLlmSettings] = useState(false);
  // backfilling はストア側では同時実行不可のグローバルフラグ（SyncLocks 共有）のため、
  // 「どのアカウントの操作でボタンを無効化するか」はローカルで追跡する
  const [backfillingAccountId, setBackfillingAccountId] = useState<string | null>(null);

  const handleBackfill = async (accountId: string) => {
    setBackfillingAccountId(accountId);
    await backfillAccount(accountId, BACKFILL_LIMIT);
    setBackfillingAccountId(null);
  };

  const handleSearch = (query: string) => {
    if (!selectedAccountId) return;
    search(selectedAccountId, query);
    setViewMode("search");
  };

  const handleClearSearch = () => {
    clearSearch();
    setViewMode("threads");
  };

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
    directoryPath?: string,
  ) => {
    if (!selectedAccountId) return;
    const project = await createProject(selectedAccountId, name, description, color);
    if (directoryPath) {
      try {
        await linkDirectory(project.id, directoryPath);
        void rescanProject(project.id);
      } catch {
        // linkDirectory は projectStore 内で errorStore へ通知済み。
        // 案件自体は作成済みなので、フォームは閉じて二重作成を防ぐ
      }
    }
    setShowProjectForm(false);
  };

  return (
    <aside className="flex h-full w-64 flex-col border-r bg-gray-50">
      <div className="flex items-center justify-between border-b px-4 py-3">
        <h1 className="text-lg font-bold">Pigeon</h1>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setShowLlmSettings(true)}
            className="rounded p-1 text-gray-500 hover:bg-gray-100"
            aria-label="LLM設定を開く"
            title="LLM設定"
          >
            ⚙️
          </button>
          <button
            onClick={() => setShowForm(!showForm)}
            className="text-sm text-blue-600 hover:underline"
          >
            {showForm ? "閉じる" : "+ 追加"}
          </button>
        </div>
      </div>
      {showForm && (
        <AccountForm
          onSubmit={handleSubmit}
          onCancel={() => setShowForm(false)}
        />
      )}
      <SearchBar onSearch={handleSearch} onClear={handleClearSearch} />
      <SearchModeToggle accountId={selectedAccountId} />
      <div className="flex-1 overflow-y-auto">
        <AccountList
          accounts={accounts}
          selectedId={selectedAccountId}
          onSelect={handleSelectAccount}
          onRemove={removeAccount}
          onReauth={startReauth}
          onBackfill={handleBackfill}
          backfillingAccountId={backfillingAccountId}
          backfillExhausted={backfillExhausted}
        />
        <ProjectTree
          onSelectUnclassified={() => setViewMode("unclassified")}
          onSelectProject={() => setViewMode("project")}
        />
        <SmartViewList accountId={selectedAccountId} />
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
      <div className="border-t">
        <button
          onClick={() => openCompose("new")}
          className="w-full px-4 py-3 text-left text-sm text-blue-600 hover:bg-gray-100 hover:underline"
        >
          ✉ 新規作成
        </button>
        {selectedAccountId && (
          <button
            onClick={() => setViewMode("drafts")}
            className="w-full px-4 py-3 text-left text-sm text-gray-700 hover:bg-gray-100"
          >
            📝 下書き
          </button>
        )}
      </div>
      <div className="border-t">
        <NotificationToggle />
      </div>
      <SyncIndicator />
      <ScanIndicator />
      {showLlmSettings && (
        <LlmSettingsDialog onClose={() => setShowLlmSettings(false)} />
      )}
    </aside>
  );
}
