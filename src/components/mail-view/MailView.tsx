import { useMailStore } from "../../stores/mailStore";
import { MailHeader } from "./MailHeader";
import { MailTabs } from "./MailTabs";
import { MailBody } from "./MailBody";
import { EmptyState } from "../common/EmptyState";

export function MailView() {
  const { selectedThread, selectedMail, selectMail } = useMailStore();

  if (!selectedThread && !selectedMail) {
    return <EmptyState message="スレッドを選択してください" />;
  }

  // Search result mode: selectedMail without a thread — skip MailTabs
  if (!selectedThread && selectedMail) {
    return (
      <div className="flex h-full flex-col">
        <MailHeader mail={selectedMail} />
        <MailBody mail={selectedMail} />
      </div>
    );
  }

  const mail =
    selectedMail ?? selectedThread!.mails[selectedThread!.mails.length - 1];

  return (
    <div className="flex h-full flex-col">
      <MailTabs
        mails={selectedThread!.mails}
        activeMailId={mail.id}
        onSelect={selectMail}
      />
      <MailHeader mail={mail} />
      <MailBody mail={mail} />
    </div>
  );
}
