import { useState } from "react";
import { useMailStore } from "../../stores/mailStore";
import { useClassifyStore } from "../../stores/classifyStore";
import { useProjectStore } from "../../stores/projectStore";
import { MailHeader } from "./MailHeader";
import { MailTabs } from "./MailTabs";
import { MailBody } from "./MailBody";
import { MailActions } from "./MailActions";
import { ClassifyApprovalBar } from "./ClassifyApprovalBar";
import { EmptyState } from "../common/EmptyState";
import { needsConfirmation } from "../../utils/classifyConfidence";
import type { Mail } from "../../types/mail";

export function MailView() {
  const selectedThread = useMailStore((s) => s.selectedThread);
  const selectedMail = useMailStore((s) => s.selectedMail);
  const selectMail = useMailStore((s) => s.selectMail);
  const projects = useProjectStore((s) => s.projects);
  const selectedProjectId = useProjectStore((s) => s.selectedProjectId);
  const approveClassification = useClassifyStore((s) => s.approveClassification);
  const [confirmingMailId, setConfirmingMailId] = useState<string | null>(null);

  if (!selectedThread && !selectedMail) {
    return <EmptyState message="スレッドを選択してください" />;
  }

  const mail: Mail =
    selectedMail ?? selectedThread!.mails[selectedThread!.mails.length - 1];

  /** 要確認のメールだけ、バッジのクリックで確認バーを開けるようにする */
  const canConfirm = needsConfirmation(mail);
  const showApprovalBar = canConfirm && confirmingMailId === mail.id;
  // 割り当て先は案件ビューなら選択中の案件。INBOX 等では確定先が一意に
  // 決まらないため「修正する」で明示的に選ばせる
  const currentProjectId = selectedProjectId ?? "";

  const approvalBar = showApprovalBar ? (
    <ClassifyApprovalBar
      projects={projects}
      currentProjectId={currentProjectId}
      onApprove={(projectId) => {
        void approveClassification(mail.id, projectId);
        setConfirmingMailId(null);
      }}
      onDismiss={() => setConfirmingMailId(null)}
    />
  ) : null;

  const header = (
    <MailHeader
      mail={mail}
      onBadgeClick={canConfirm ? () => setConfirmingMailId(mail.id) : undefined}
    />
  );

  // Search result mode: selectedMail without a thread — skip MailTabs
  if (!selectedThread) {
    return (
      <div className="flex h-full flex-col">
        {header}
        {approvalBar}
        <MailActions mail={mail} />
        <MailBody mail={mail} />
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <MailTabs
        mails={selectedThread.mails}
        activeMailId={mail.id}
        onSelect={selectMail}
      />
      {header}
      {approvalBar}
      <MailActions mail={mail} />
      <MailBody mail={mail} />
    </div>
  );
}
