import { useEffect } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useMailStore } from "../../stores/mailStore";
import { useProjectStore } from "../../stores/projectStore";
import { useSelectionStore } from "../../stores/selectionStore";
import { ThreadItem } from "./ThreadItem";
import { BulkActionBar } from "./BulkActionBar";
import { EmptyState } from "../common/EmptyState";
import { useDisplayLimit } from "../../hooks/useDisplayLimit";
import { useBulkActions } from "../../hooks/useBulkActions";
import { INBOX_FOLDER } from "../../constants/folders";

interface ThreadListProps {
  viewMode: "threads" | "project";
}

export function ThreadList({ viewMode }: ThreadListProps) {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const startReauth = useAccountStore((s) => s.startReauth);
  const selectedProjectId = useProjectStore((s) => s.selectedProjectId);
  const projects = useProjectStore((s) => s.projects);
  const threads = useMailStore((s) => s.threads);
  const syncing = useMailStore((s) => s.syncing);
  const needsReauth = useMailStore((s) => s.needsReauth);
  const selectedThread = useMailStore((s) => s.selectedThread);
  const fetchThreads = useMailStore((s) => s.fetchThreads);
  const fetchThreadsByProject = useMailStore((s) => s.fetchThreadsByProject);
  const syncAccount = useMailStore((s) => s.syncAccount);
  const selectThread = useMailStore((s) => s.selectThread);
  const clearSelection = useSelectionStore((s) => s.clear);
  const { visible, hasMore, remaining, showMore } = useDisplayLimit(
    threads,
    `${viewMode}:${selectedProjectId ?? ""}:${selectedAccountId ?? ""}`,
  );

  useEffect(() => {
    let cancelled = false;
    if (viewMode === "project" && selectedProjectId) {
      void fetchThreadsByProject(selectedProjectId);
    } else if (viewMode === "threads" && selectedAccountId) {
      void syncAccount(selectedAccountId).then(() => {
        // 高速切替で古いアカウントの結果が新しい一覧を上書きしないよう、
        // クリーンアップ済みなら取得しない。再認証が必要なときも取得は
        // 無意味なためスキップする（再ログイン導線を表示する）
        if (cancelled || useMailStore.getState().needsReauth) return;
        void fetchThreads(selectedAccountId, INBOX_FOLDER);
      });
    }
    clearSelection();
    return () => {
      cancelled = true;
    };
  }, [viewMode, selectedAccountId, selectedProjectId, fetchThreads, fetchThreadsByProject, syncAccount, clearSelection]);

  const reloadThreads = () => {
    if (viewMode === "project" && selectedProjectId) {
      void fetchThreadsByProject(selectedProjectId);
    } else if (selectedAccountId) {
      void fetchThreads(selectedAccountId, INBOX_FOLDER);
    }
  };

  const { handleBulkDelete, handleBulkArchive, handleBulkMove, selectedCount } =
    useBulkActions({
      accountId: selectedAccountId,
      threads,
      reload: reloadThreads,
    });

  if (!selectedAccountId) {
    return <EmptyState message="アカウントを選択してください" />;
  }
  if (needsReauth && selectedAccountId) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 px-4">
        <p className="text-sm text-amber-600">
          認証の有効期限が切れました。再ログインしてください。
        </p>
        <button
          onClick={() => startReauth(selectedAccountId)}
          className="rounded bg-blue-600 px-4 py-2 text-sm text-white hover:bg-blue-700"
        >
          再ログイン
        </button>
      </div>
    );
  }
  if (syncing && threads.length === 0) {
    return <EmptyState message="メールを同期中..." />;
  }
  if (threads.length === 0) {
    return <EmptyState message="メールがありません" />;
  }
  return (
    <div className="flex h-full flex-col">
      <BulkActionBar
        selectedCount={selectedCount}
        projects={projects}
        onDelete={() => void handleBulkDelete()}
        onArchive={() => void handleBulkArchive()}
        onMove={(projectId) => void handleBulkMove(projectId)}
        onClear={clearSelection}
      />
      <div className="flex-1 overflow-y-auto">
        {visible.map((thread) => (
          <ThreadItem
            key={thread.thread_id}
            thread={thread}
            selected={selectedThread?.thread_id === thread.thread_id}
            onClick={() => selectThread(thread)}
          />
        ))}
        {hasMore && (
          <button
            onClick={showMore}
            className="w-full py-2 text-sm text-blue-600 hover:bg-gray-50"
          >
            もっと見る（残り {remaining.toLocaleString()} 件）
          </button>
        )}
      </div>
    </div>
  );
}
