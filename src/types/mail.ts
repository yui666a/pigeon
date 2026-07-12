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
  is_flagged: boolean;
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

/** 一括操作（bulk_delete_mails 等）の結果。1件の失敗で残りは止めない */
export interface BulkResult {
  succeeded: string[];
  /** [mail_id, エラーメッセージ] の組 */
  failed: [string, string][];
}
