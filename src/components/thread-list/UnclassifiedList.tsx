import { useEffect, useRef } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useClassifyStore } from "../../stores/classifyStore";
import { useMailStore } from "../../stores/mailStore";
import { useProjectStore } from "../../stores/projectStore";
import { useSelectionStore } from "../../stores/selectionStore";
import { ClassifyButton } from "./ClassifyButton";
import { ThreadDragItem } from "./ThreadDragItem";
import { BulkActionBar } from "./BulkActionBar";
import { NewProjectProposal } from "../common/NewProjectProposal";
import { useDisplayLimit } from "../../hooks/useDisplayLimit";
import type { Thread } from "../../types/mail";

export function UnclassifiedList() {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const pendingProposal = useClassifyStore((s) => s.pendingProposal);
  const classifying = useClassifyStore((s) => s.classifying);
  const approveNewProjectStore = useClassifyStore((s) => s.approveNewProject);
  const rejectClassification = useClassifyStore(
    (s) => s.rejectClassification,
  );
  const removeUnclassifiedMail = useMailStore((s) => s.removeUnclassifiedMail);
  const unclassifiedMails = useMailStore((s) => s.unclassifiedMails);
  const unclassifiedThreads = useMailStore((s) => s.unclassifiedThreads);
  const fetchUnclassified = useMailStore((s) => s.fetchUnclassified);
  const selectThread = useMailStore((s) => s.selectThread);
  const bulkDeleteMails = useMailStore((s) => s.bulkDeleteMails);
  const bulkArchiveMails = useMailStore((s) => s.bulkArchiveMails);
  const bulkMoveMails = useMailStore((s) => s.bulkMoveMails);
  const projects = useProjectStore((s) => s.projects);
  const selectedThreadIds = useSelectionStore((s) => s.selectedThreadIds);
  const selectedMailIds = useSelectionStore((s) => s.selectedMailIds);
  const clearSelection = useSelectionStore((s) => s.clear);
  const {
    visible: visibleThreads,
    hasMore,
    remaining,
    showMore,
  } = useDisplayLimit(unclassifiedThreads, selectedAccountId);

  useEffect(() => {
    if (selectedAccountId) {
      fetchUnclassified(selectedAccountId);
    }
  }, [selectedAccountId, fetchUnclassified]);

  // 分類完了エッジ（classifying: true → false）でのみ再取得する。
  // 初回マウントやアカウント切り替えの取得は上の effect が担うため、
  // ここで無条件に取得すると二重発火になる
  const prevClassifying = useRef(classifying);
  useEffect(() => {
    const wasClassifying = prevClassifying.current;
    prevClassifying.current = classifying;
    if (wasClassifying && !classifying && selectedAccountId) {
      fetchUnclassified(selectedAccountId);
    }
  }, [classifying, selectedAccountId, fetchUnclassified]);

  if (!selectedAccountId) return null;

  const handleApproveNewProject = async (mailId: string, projectName: string, description?: string) => {
    await approveNewProjectStore(mailId, projectName, description);
    removeUnclassifiedMail(mailId);
  };

  const handleThreadClick = (thread: Thread) => {
    // 実スレッドを渡すことで MailView にスレッド内タブが表示される
    selectThread(thread);
  };

  const handleBulkDelete = async () => {
    const mailIds = selectedMailIds(unclassifiedThreads);
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
    void fetchUnclassified(selectedAccountId);
  };

  const handleBulkArchive = async () => {
    const mailIds = selectedMailIds(unclassifiedThreads);
    if (mailIds.length === 0) return;
    await bulkArchiveMails(selectedAccountId, mailIds);
    clearSelection();
    void fetchUnclassified(selectedAccountId);
  };

  const handleBulkMove = async (projectId: string) => {
    const mailIds = selectedMailIds(unclassifiedThreads);
    if (mailIds.length === 0) return;
    await bulkMoveMails(mailIds, projectId);
    clearSelection();
    void fetchUnclassified(selectedAccountId);
  };

  return (
    <div className="border-b">
      <div className="flex items-center justify-between px-4 py-2">
        <h3 className="text-sm font-medium text-gray-700">
          未分類メール ({unclassifiedMails.length})
        </h3>
      </div>

      <ClassifyButton accountId={selectedAccountId} />

      {pendingProposal && pendingProposal.action === "create" && (
        <div className="space-y-2 px-4 pb-2">
          <NewProjectProposal
            key={pendingProposal.mail_id}
            mailId={pendingProposal.mail_id}
            suggestedName={pendingProposal.project_name ?? ""}
            suggestedDescription={pendingProposal.description}
            reason={pendingProposal.reason}
            onApprove={handleApproveNewProject}
            onReject={rejectClassification}
          />
        </div>
      )}

      {unclassifiedThreads.length > 0 && (
        <div>
          <BulkActionBar
            selectedCount={selectedThreadIds.size}
            projects={projects}
            onDelete={() => void handleBulkDelete()}
            onArchive={() => void handleBulkArchive()}
            onMove={(projectId) => void handleBulkMove(projectId)}
            onClear={clearSelection}
          />
          <div className="max-h-48 overflow-y-auto">
            {visibleThreads.map((thread) => (
              <ThreadDragItem
                key={thread.thread_id}
                thread={thread}
                onClick={() => handleThreadClick(thread)}
              />
            ))}
            {hasMore && (
              <button
                onClick={showMore}
                className="w-full py-2 text-xs text-blue-600 hover:bg-gray-50"
              >
                もっと見る（残り {remaining.toLocaleString()} 件）
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
