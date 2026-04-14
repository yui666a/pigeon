import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAccountStore } from "../../stores/accountStore";
import { useMailStore } from "../../stores/mailStore";
import { useProjectStore } from "../../stores/projectStore";
import { ThreadItem } from "./ThreadItem";
import type { Thread } from "../../types/mail";

interface ThreadListProps {
  viewMode: "threads" | "project";
}

export function ThreadList({ viewMode }: ThreadListProps) {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const selectedProjectId = useProjectStore((s) => s.selectedProjectId);
  const { threads, syncing, selectedThread, fetchThreads, syncAccount, selectThread, setThreads } =
    useMailStore();

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
  }, [viewMode, selectedAccountId, selectedProjectId, fetchThreads, syncAccount, setThreads]);

  if (!selectedAccountId) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-gray-400">
        アカウントを選択してください
      </div>
    );
  }
  if (syncing) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-gray-400">
        メールを同期中...
      </div>
    );
  }
  if (threads.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-gray-400">
        メールがありません
      </div>
    );
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
