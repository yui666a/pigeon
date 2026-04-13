import { useEffect } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useClassifyStore } from "../../stores/classifyStore";
import { ClassifyButton } from "./ClassifyButton";
import { NewProjectProposal } from "../common/NewProjectProposal";

export function UnclassifiedList() {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const unclassifiedMails = useClassifyStore((s) => s.unclassifiedMails);
  const results = useClassifyStore((s) => s.results);
  const summary = useClassifyStore((s) => s.summary);
  const fetchUnclassified = useClassifyStore((s) => s.fetchUnclassified);
  const approveNewProject = useClassifyStore((s) => s.approveNewProject);
  const rejectClassification = useClassifyStore(
    (s) => s.rejectClassification,
  );
  const initClassifyListeners = useClassifyStore(
    (s) => s.initClassifyListeners,
  );

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

  if (!selectedAccountId) return null;

  const createResults = results.filter((r) => r.action === "create");

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
              onApprove={approveNewProject}
              onReject={rejectClassification}
            />
          ))}
        </div>
      )}

      {unclassifiedMails.length > 0 && (
        <div className="max-h-48 overflow-y-auto">
          {unclassifiedMails.map((mail) => (
            <div key={mail.id} className="border-t px-4 py-2">
              <div className="truncate text-sm">{mail.subject}</div>
              <div className="truncate text-xs text-gray-500">
                {mail.from_addr}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
