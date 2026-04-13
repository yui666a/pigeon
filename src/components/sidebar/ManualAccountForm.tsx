import { useState } from "react";
import type { CreateAccountRequest } from "../../types/account";

interface ManualAccountFormProps {
  onSubmit: (req: CreateAccountRequest) => void;
  onBack: () => void;
}

export function ManualAccountForm({ onSubmit, onBack }: ManualAccountFormProps) {
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [imapHost, setImapHost] = useState("");
  const [imapPort, setImapPort] = useState(993);
  const [smtpHost, setSmtpHost] = useState("");
  const [smtpPort, setSmtpPort] = useState(587);
  const [password, setPassword] = useState("");

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    onSubmit({
      name,
      email,
      imap_host: imapHost,
      imap_port: imapPort,
      smtp_host: smtpHost,
      smtp_port: smtpPort,
      auth_type: "plain",
      password,
    });
  };

  return (
    <form onSubmit={handleSubmit} className="flex flex-col gap-3 p-4">
      <label className="flex flex-col gap-1">
        <span className="text-sm text-gray-600">アカウント名</span>
        <input
          aria-label="アカウント名"
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="rounded border px-2 py-1 text-sm"
          required
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-sm text-gray-600">メールアドレス</span>
        <input
          aria-label="メールアドレス"
          type="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          className="rounded border px-2 py-1 text-sm"
          required
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-sm text-gray-600">IMAPサーバー</span>
        <input
          aria-label="IMAPサーバー"
          type="text"
          value={imapHost}
          onChange={(e) => setImapHost(e.target.value)}
          className="rounded border px-2 py-1 text-sm"
          required
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-sm text-gray-600">IMAPポート</span>
        <input
          aria-label="IMAPポート"
          type="number"
          value={imapPort}
          onChange={(e) => setImapPort(Number(e.target.value))}
          className="rounded border px-2 py-1 text-sm"
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-sm text-gray-600">SMTPサーバー</span>
        <input
          aria-label="SMTPサーバー"
          type="text"
          value={smtpHost}
          onChange={(e) => setSmtpHost(e.target.value)}
          className="rounded border px-2 py-1 text-sm"
          required
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-sm text-gray-600">SMTPポート</span>
        <input
          aria-label="SMTPポート"
          type="number"
          value={smtpPort}
          onChange={(e) => setSmtpPort(Number(e.target.value))}
          className="rounded border px-2 py-1 text-sm"
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-sm text-gray-600">パスワード</span>
        <input
          aria-label="パスワード"
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className="rounded border px-2 py-1 text-sm"
          required
        />
      </label>
      <div className="flex gap-2">
        <button
          type="submit"
          className="rounded bg-blue-600 px-4 py-1 text-sm text-white hover:bg-blue-700"
        >
          追加
        </button>
        <button
          type="button"
          onClick={onBack}
          className="rounded border px-4 py-1 text-sm hover:bg-gray-100"
        >
          戻る
        </button>
      </div>
    </form>
  );
}
