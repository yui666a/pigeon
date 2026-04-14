import type { Mail } from "../../types/mail";

interface MailTabsProps {
  mails: Mail[];
  activeMailId: string;
  onSelect: (mail: Mail) => void;
}

export function MailTabs({ mails, activeMailId, onSelect }: MailTabsProps) {
  if (mails.length <= 1) return null;

  return (
    <div className="flex gap-1 border-b px-4 py-2">
      {mails.map((m, i) => (
        <button
          key={m.id}
          onClick={() => onSelect(m)}
          className={`rounded px-2 py-1 text-xs ${m.id === activeMailId ? "bg-blue-100 text-blue-700" : "hover:bg-gray-100"}`}
        >
          {i + 1}
        </button>
      ))}
    </div>
  );
}
