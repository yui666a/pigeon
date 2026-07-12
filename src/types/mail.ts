export interface Mail {
  id: string;
  account_id: string;
  folder: string;
  message_id: string;
  in_reply_to: string | null;
  references: string | null;
  from_addr: string;
  to_addr: string;
  cc_addr: string | null;
  subject: string;
  body_text: string | null;
  body_html: string | null;
  date: string;
  has_attachments: boolean;
  raw_size: number | null;
  uid: number;
  flags: string | null;
  is_read: boolean;
  fetched_at: string;
}

export interface UnreadCounts {
  /** project_id → 未読件数 */
  by_project: Record<string, number>;
  /** 未分類メールの未読件数 */
  unclassified: number;
}

/** Tauri `send_mail` command の入力（Rust側 SendMailRequest と対応） */
export interface SendMailRequest {
  account_id: string;
  to: string[];
  cc: string[];
  bcc: string[];
  subject: string;
  body_text: string;
  reply_to_mail_id: string | null;
}

/** ローカル下書き（v1: IMAP Draftsフォルダとの同期は将来） */
export interface Draft {
  id: string;
  account_id: string;
  to_addr: string;
  cc_addr: string;
  bcc_addr: string;
  subject: string;
  body_text: string;
  in_reply_to: string | null;
  created_at: string;
  updated_at: string;
}

/** Tauri `save_draft` command の入力（Rust側 SaveDraftRequest と対応） */
export interface SaveDraftRequest {
  id: string | null;
  account_id: string;
  to_addr: string;
  cc_addr: string;
  bcc_addr: string;
  subject: string;
  body_text: string;
  in_reply_to: string | null;
}

export interface SearchResult {
  mail: Mail;
  project_id: string | null;
  project_name: string | null;
  snippet: string;
}

export interface Thread {
  thread_id: string;
  subject: string;
  last_date: string;
  mail_count: number;
  from_addrs: string[];
  mails: Mail[];
}
