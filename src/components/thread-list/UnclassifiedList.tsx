import { useEffect } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useClassifyStore } from "../../stores/classifyStore";
import { useMailStore } from "../../stores/mailStore";
import { ClassifyButton } from "./ClassifyButton";
import { MailDragItem } from "./MailDragItem";
import { NewProjectProposal } from "../common/NewProjectProposal";
import { useDisplayLimit } from "../../hooks/useDisplayLimit";
import type { Mail } from "../../types/mail";
import { threadFromMail } from "../../utils/thread";

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
  const fetchUnclassified = useMailStore((s) => s.fetchUnclassified);
  const { selectThread, selectMail } = useMailStore();
  const {
    visible: visibleMails,
    hasMore,
    remaining,
    showMore,
  } = useDisplayLimit(unclassifiedMails, selectedAccountId);

  useEffect(() => {
    if (selectedAccountId) {
      fetchUnclassified(selectedAccountId);
    }
  }, [selectedAccountId, fetchUnclassified]);

  useEffect(() => {
    if (!classifying && selectedAccountId) {
      fetchUnclassified(selectedAccountId);
    }
  }, [classifying, selectedAccountId, fetchUnclassified]);

  if (!selectedAccountId) return null;

  const handleApproveNewProject = async (mailId: string, projectName: string, description?: string) => {
    await approveNewProjectStore(mailId, projectName, description);
    removeUnclassifiedMail(mailId);
  };

  const handleMailClick = (mail: Mail) => {
    selectThread(threadFromMail(mail));
    selectMail(mail);
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

      {unclassifiedMails.length > 0 && (
        <div className="max-h-48 overflow-y-auto">
          {visibleMails.map((mail) => (
            <MailDragItem
              key={mail.id}
              mail={mail}
              onClick={() => handleMailClick(mail)}
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
      )}
    </div>
  );
}
