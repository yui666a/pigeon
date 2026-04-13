import { useMailStore } from "../../stores/mailStore";
import { MailHeader } from "./MailHeader";

export function MailView() {
  const { selectedThread, selectedMail, selectMail } = useMailStore();

  if (!selectedThread) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-gray-400">
        スレッドを選択してください
      </div>
    );
  }

  const mail =
    selectedMail ?? selectedThread.mails[selectedThread.mails.length - 1];

  return (
    <div className="flex h-full flex-col">
      {selectedThread.mails.length > 1 && (
        <div className="flex gap-1 border-b px-4 py-2">
          {selectedThread.mails.map((m, i) => (
            <button
              key={m.id}
              onClick={() => selectMail(m)}
              className={`rounded px-2 py-1 text-xs ${m.id === mail.id ? "bg-blue-100 text-blue-700" : "hover:bg-gray-100"}`}
            >
              {i + 1}
            </button>
          ))}
        </div>
      )}
      <MailHeader mail={mail} />
      <div className="flex-1 overflow-y-auto px-6 py-4">
        {mail.body_html ? (
          <div
            className="prose max-w-none text-sm"
            dangerouslySetInnerHTML={{ __html: mail.body_html }}
          />
        ) : (
          <pre className="whitespace-pre-wrap text-sm">{mail.body_text}</pre>
        )}
      </div>
    </div>
  );
}
