import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAccountStore } from "../../stores/accountStore";
import { useMailStore } from "../../stores/mailStore";
import { useProjectStore } from "../../stores/projectStore";
import { ThreadItem } from "./ThreadItem";
import { EmptyState } from "../common/EmptyState";
import type { Thread } from "../../types/mail";

interface ThreadListProps {
  viewMode: "threads" | "project";
}

export function ThreadList({ viewMode }: ThreadListProps) {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const startReauth = useAccountStore((s) => s.startReauth);
  const selectedProjectId = useProjectStore((s) => s.selectedProjectId);
  const {
    threads,
    syncing,
    needsReauthAccountId,
    selectedThread,
    fetchThreads,
    syncAccount,
    selectThread,
    setThreads,
  } =
    useMailStore();
  const needsReauth =
    selectedAccountId !== null && needsReauthAccountId === selectedAccountId;

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
      if (needsReauth) {
        return;
      }
      syncAccount(selectedAccountId).then(() => {
        fetchThreads(selectedAccountId, "INBOX");
      });
    }
  }, [
    viewMode,
    selectedAccountId,
    selectedProjectId,
    needsReauth,
    fetchThreads,
    syncAccount,
    setThreads,
  ]);

  if (!selectedAccountId) {
    return <EmptyState message="アカウントを選択してください" />;
  }
  if (needsReauth) {
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
  if (syncing) {
    return <EmptyState message="メールを同期中..." />;
  }
  if (threads.length === 0) {
    return <EmptyState message="メールがありません" />;
  }
  return (
    <div className="h-full overflow-y-auto">
      {threads.map((thread) => (
        <ThreadItem
          key={thread.thread_id}
          thread={thread}
          selected={selectedThread?.thread_id === thread.thread_id}
          onClick={() => selectThread(thread)}
        />
      ))}
    </div>
  );
}
