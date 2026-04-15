import { useEffect } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useClassifyStore } from "../../stores/classifyStore";
import { useProjectStore } from "../../stores/projectStore";
import { useMailStore } from "../../stores/mailStore";
import { ClassifyButton } from "./ClassifyButton";
import { MailDragItem } from "./MailDragItem";
import { NewProjectProposal } from "../common/NewProjectProposal";
import type { Mail } from "../../types/mail";
import { threadFromMail } from "../../utils/thread";

export function UnclassifiedList() {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const results = useClassifyStore((s) => s.results);
  const summary = useClassifyStore((s) => s.summary);
  const classifying = useClassifyStore((s) => s.classifying);
  const approveNewProjectStore = useClassifyStore((s) => s.approveNewProject);
  const rejectClassification = useClassifyStore(
    (s) => s.rejectClassification,
  );
  const removeUnclassifiedMail = useMailStore((s) => s.removeUnclassifiedMail);
  const initClassifyListeners = useClassifyStore(
    (s) => s.initClassifyListeners,
  );
  const fetchProjects = useProjectStore((s) => s.fetchProjects);
  const unclassifiedMails = useMailStore((s) => s.unclassifiedMails);
  const fetchUnclassified = useMailStore((s) => s.fetchUnclassified);
  const { selectThread, selectMail } = useMailStore();

  useEffect(() => {
    if (selectedAccountId) {
      fetchUnclassified(selectedAccountId);
    }
  }, [selectedAccountId, fetchUnclassified]);

  useEffect(() => {
    const promise = initClassifyListeners();
    return () => {
      promise.then((unlisten) => unlisten());
    };
  }, [initClassifyListeners]);

  useEffect(() => {
    if (!classifying && summary && selectedAccountId) {
      fetchProjects(selectedAccountId);
      fetchUnclassified(selectedAccountId);
    }
  }, [classifying, summary, selectedAccountId, fetchProjects, fetchUnclassified]);

  if (!selectedAccountId) return null;

  const handleApproveNewProject = async (mailId: string, projectName: string, description?: string) => {
    await approveNewProjectStore(mailId, projectName, description);
    removeUnclassifiedMail(mailId);
  };

  const createResults = results.filter((r) => r.action === "create");

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

      {summary && (
        <div className="mx-4 mb-2 rounded bg-gray-50 p-2 text-xs text-gray-600">
          <span>合計: {summary.total}</span>
          <span className="ml-2">分類済: {summary.assigned}</span>
          <span className="ml-2">要確認: {summary.needs_review}</span>
          <span className="ml-2">未分類: {summary.unclassified}</span>
        </div>
      )}

      {createResults.length > 0 && (
        <div className="space-y-2 px-4 pb-2">
          {createResults.map((result) => (
            <NewProjectProposal
              key={result.mail_id}
              mailId={result.mail_id}
              suggestedName={result.project_name ?? ""}
              suggestedDescription={result.description}
              reason={result.reason}
              onApprove={handleApproveNewProject}
              onReject={rejectClassification}
            />
          ))}
        </div>
      )}

      {unclassifiedMails.length > 0 && (
        <div className="max-h-48 overflow-y-auto">
          {unclassifiedMails.map((mail) => (
            <MailDragItem
              key={mail.id}
              mail={mail}
              onClick={() => handleMailClick(mail)}
            />
          ))}
        </div>
      )}
    </div>
  );
}
