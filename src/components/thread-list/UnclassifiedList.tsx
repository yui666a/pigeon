import { useEffect, useRef } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { useClassifyStore } from "../../stores/classifyStore";
import { useProjectStore } from "../../stores/projectStore";
import { useMailStore } from "../../stores/mailStore";
import { useDragStore } from "../../stores/dragStore";
import { ClassifyButton } from "./ClassifyButton";
import { NewProjectProposal } from "../common/NewProjectProposal";
import type { Mail } from "../../types/mail";

function MailDragItem({ mail, onClick }: { mail: Mail; onClick: () => void }) {
  const startDrag = useDragStore((s) => s.startDrag);
  const updatePosition = useDragStore((s) => s.updatePosition);
  const isDragging = useRef(false);
  const startPos = useRef({ x: 0, y: 0 });

  const handleMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    startPos.current = { x: e.clientX, y: e.clientY };
    isDragging.current = false;

    const handleMouseMove = (me: MouseEvent) => {
      const dx = me.clientX - startPos.current.x;
      const dy = me.clientY - startPos.current.y;
      if (!isDragging.current && Math.abs(dx) + Math.abs(dy) > 5) {
        isDragging.current = true;
        startDrag([mail.id], mail.subject);
        updatePosition(me.clientX, me.clientY);
      }
      if (isDragging.current) {
        updatePosition(me.clientX, me.clientY);
      }
    };

    const handleMouseUp = () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
      if (!isDragging.current) {
        onClick();
      }
      isDragging.current = false;
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
  };

  return (
    <div
      onMouseDown={handleMouseDown}
      className="w-full cursor-pointer border-t px-4 py-2 text-left hover:bg-gray-50"
    >
      <div className="truncate text-sm">{mail.subject}</div>
      <div className="truncate text-xs text-gray-500">{mail.from_addr}</div>
    </div>
  );
}

export function UnclassifiedList() {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const unclassifiedMails = useClassifyStore((s) => s.unclassifiedMails);
  const results = useClassifyStore((s) => s.results);
  const summary = useClassifyStore((s) => s.summary);
  const classifying = useClassifyStore((s) => s.classifying);
  const fetchUnclassified = useClassifyStore((s) => s.fetchUnclassified);
  const approveNewProject = useClassifyStore((s) => s.approveNewProject);
  const rejectClassification = useClassifyStore(
    (s) => s.rejectClassification,
  );
  const initClassifyListeners = useClassifyStore(
    (s) => s.initClassifyListeners,
  );
  const fetchProjects = useProjectStore((s) => s.fetchProjects);
  const { selectThread, selectMail } = useMailStore();
  const startDrag = useDragStore((s) => s.startDrag);
  const endDrag = useDragStore((s) => s.endDrag);

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
    }
  }, [classifying, summary, selectedAccountId, fetchProjects]);

  if (!selectedAccountId) return null;

  const createResults = results.filter((r) => r.action === "create");

  const handleMailClick = (mail: Mail) => {
    selectThread({
      thread_id: mail.message_id || mail.id,
      subject: mail.subject,
      last_date: mail.date,
      mail_count: 1,
      from_addrs: [mail.from_addr],
      mails: [mail],
    });
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
              onApprove={approveNewProject}
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
