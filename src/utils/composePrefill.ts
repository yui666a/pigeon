import type { Mail } from "../types/mail";
import { formatFullDate } from "./date";

export type ComposeMode = "new" | "reply" | "replyAll" | "forward";

/** Compose画面の初期値。宛先はカンマ区切り文字列としてUIで保持する */
export interface ComposePrefill {
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  body: string;
}

const EMPTY_PREFILL: ComposePrefill = {
  to: "",
  cc: "",
  bcc: "",
  subject: "",
  body: "",
};

/** カンマ区切りの宛先文字列を配列へ分割する（trim・空要素除去） */
export function splitRecipients(value: string): string[] {
  return value
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

/** "Name <a@b.com>" 形式にも対応してメールアドレス部分を小文字で取り出す */
function emailOf(addr: string): string {
  const match = addr.match(/<([^>]+)>/);
  return (match ? match[1] : addr).trim().toLowerCase();
}

/** 自分のアドレス（大文字小文字無視）を除外する */
function excludeSelf(addrs: string[], selfEmail: string | null): string[] {
  if (!selfEmail) return addrs;
  const self = selfEmail.trim().toLowerCase();
  return addrs.filter((a) => emailOf(a) !== self);
}

/** 既に同じプレフィックスで始まる場合は付け直さない（大文字小文字無視） */
function prefixSubject(subject: string, prefix: "Re: " | "Fwd: "): string {
  const pattern = new RegExp(`^${prefix.trimEnd()}`, "i");
  return pattern.test(subject.trimStart()) ? subject : `${prefix}${subject}`;
}

/** `{date} {from}:` ヘッダー + 各行 `> ` プレフィックスの引用ブロック */
function quoteBody(mail: Mail): string {
  const header = `${formatFullDate(mail.date)} ${mail.from_addr}:`;
  const quoted = (mail.body_text ?? "")
    .split("\n")
    .map((line) => `> ${line}`)
    .join("\n");
  return `\n\n${header}\n${quoted}`;
}

/**
 * モードと元メールからComposeの初期値を組み立てる純関数。
 * accountEmail はreplyAllの自分除外に使う（未選択時はnull）
 */
export function buildPrefill(
  mode: ComposeMode,
  sourceMail: Mail | null,
  accountEmail: string | null,
): ComposePrefill {
  if (mode === "new" || !sourceMail) {
    return { ...EMPTY_PREFILL };
  }

  if (mode === "reply") {
    return {
      ...EMPTY_PREFILL,
      to: sourceMail.from_addr,
      subject: prefixSubject(sourceMail.subject, "Re: "),
      body: quoteBody(sourceMail),
    };
  }

  if (mode === "replyAll") {
    const to = excludeSelf(
      [sourceMail.from_addr, ...splitRecipients(sourceMail.to_addr)],
      accountEmail,
    );
    const cc = excludeSelf(
      splitRecipients(sourceMail.cc_addr ?? ""),
      accountEmail,
    );
    return {
      ...EMPTY_PREFILL,
      to: to.join(", "),
      cc: cc.join(", "),
      subject: prefixSubject(sourceMail.subject, "Re: "),
      body: quoteBody(sourceMail),
    };
  }

  // forward
  return {
    ...EMPTY_PREFILL,
    subject: prefixSubject(sourceMail.subject, "Fwd: "),
    body: quoteBody(sourceMail),
  };
}
