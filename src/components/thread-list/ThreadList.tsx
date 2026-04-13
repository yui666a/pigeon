import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAccountStore } from "../../stores/accountStore";
import { useMailStore } from "../../stores/mailStore";
import { useProjectStore } from "../../stores/projectStore";
import { ThreadItem } from "./ThreadItem";
import type { Mail, Thread } from "../../types/mail";

interface ThreadListProps {
  viewMode: "threads" | "project";
}

export function ThreadList({ viewMode }: ThreadListProps) {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const selectedProjectId = useProjectStore((s) => s.selectedProjectId);
  const { threads, selectedThread, fetchThreads, selectThread } =
    useMailStore();

  useEffect(() => {
    if (viewMode === "project" && selectedProjectId) {
      invoke<Mail[]>("get_mails_by_project", { projectId: selectedProjectId })
        .then((mails) => {
          const projectThreads: Thread[] = mails.map((mail) => ({
            thread_id: mail.message_id || mail.id,
            subject: mail.subject,
            last_date: mail.date,
            mail_count: 1,
            from_addrs: [mail.from_addr],
            mails: [mail],
          }));
          useMailStore.setState({ threads: projectThreads });
        })
        .catch(() => {
          useMailStore.setState({ threads: [] });
        });
    } else if (viewMode === "threads" && selectedAccountId) {
      fetchThreads(selectedAccountId, "INBOX");
    }
  }, [viewMode, selectedAccountId, selectedProjectId, fetchThreads]);

  if (!selectedAccountId) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-gray-400">
        アカウントを選択してください
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
