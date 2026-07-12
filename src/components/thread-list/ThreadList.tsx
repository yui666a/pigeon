import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAccountStore } from "../../stores/accountStore";
import { useMailStore } from "../../stores/mailStore";
import { useProjectStore } from "../../stores/projectStore";
import { useSelectionStore } from "../../stores/selectionStore";
import { ThreadItem } from "./ThreadItem";
import { BulkActionBar } from "./BulkActionBar";
import { EmptyState } from "../common/EmptyState";
import { useDisplayLimit } from "../../hooks/useDisplayLimit";
import type { Thread } from "../../types/mail";

interface ThreadListProps {
  viewMode: "threads" | "project";
}

export function ThreadList({ viewMode }: ThreadListProps) {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const startReauth = useAccountStore((s) => s.startReauth);
  const selectedProjectId = useProjectStore((s) => s.selectedProjectId);
  const projects = useProjectStore((s) => s.projects);
  const { threads, syncing, needsReauth, selectedThread, fetchThreads, syncAccount, selectThread, setThreads, bulkDeleteMails, bulkArchiveMails, bulkMoveMails } =
    useMailStore();
  const selectedThreadIds = useSelectionStore((s) => s.selectedThreadIds);
  const selectedMailIds = useSelectionStore((s) => s.selectedMailIds);
  const clearSelection = useSelectionStore((s) => s.clear);
  const { visible, hasMore, remaining, showMore } = useDisplayLimit(
    threads,
    `${viewMode}:${selectedProjectId ?? ""}:${selectedAccountId ?? ""}`,
  );

  useEffect(() => {
    if (viewMode === "project" && selectedProjectId) {
      invoke<Thread[]>("get_threads_by_project", { projectId: selectedProjectId })
        .then((projectThreads) => {
          setThreads(projectThreads);
        })
        .catch(() => {
          setThreads([]);
        });
    } else if (viewMode === "threads" && selectedAccountId) {
      syncAccount(selectedAccountId).then(() => {
        fetchThreads(selectedAccountId, "INBOX");
      });
    }
    clearSelection();
  }, [viewMode, selectedAccountId, selectedProjectId, fetchThreads, syncAccount, setThreads, clearSelection]);

  const reloadThreads = () => {
    if (viewMode === "project" && selectedProjectId) {
      invoke<Thread[]>("get_threads_by_project", { projectId: selectedProjectId })
        .then(setThreads)
        .catch(() => setThreads([]));
    } else if (selectedAccountId) {
      void fetchThreads(selectedAccountId, "INBOX");
    }
  };

  const handleBulkDelete = async () => {
    if (!selectedAccountId) return;
    const mailIds = selectedMailIds(threads);
    if (mailIds.length === 0) return;
    if (
      !window.confirm(
        `選択した ${selectedThreadIds.size} スレッドを削除しますか？サーバーにゴミ箱があればゴミ箱へ移動し、無い場合は完全に削除されます。`,
      )
    ) {
      return;
    }
    await bulkDeleteMails(selectedAccountId, mailIds);
    clearSelection();
    reloadThreads();
  };

  const handleBulkArchive = async () => {
    if (!selectedAccountId) return;
    const mailIds = selectedMailIds(threads);
    if (mailIds.length === 0) return;
    await bulkArchiveMails(selectedAccountId, mailIds);
    clearSelection();
    reloadThreads();
  };

  const handleBulkMove = async (projectId: string) => {
    const mailIds = selectedMailIds(threads);
    if (mailIds.length === 0) return;
    await bulkMoveMails(mailIds, projectId);
    clearSelection();
    reloadThreads();
  };

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
        selectedCount={selectedThreadIds.size}
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
