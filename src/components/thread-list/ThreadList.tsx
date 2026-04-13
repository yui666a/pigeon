import { useEffect } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useMailStore } from "../../stores/mailStore";
import { ThreadItem } from "./ThreadItem";

export function ThreadList() {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const { threads, selectedThread, fetchThreads, selectThread } =
    useMailStore();

  useEffect(() => {
    if (selectedAccountId) {
      fetchThreads(selectedAccountId, "INBOX");
    }
  }, [selectedAccountId, fetchThreads]);

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
