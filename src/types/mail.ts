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
  /** 以下2つは mails テーブルではなく mail_project_assignments 由来の注釈。
   * 未割り当てのメールでは null。確信度が中程度のAI分類に ⚠ を出すために使う */
  assigned_by?: string | null;
  confidence?: number | null;
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
  /** リッチ本文のHTML。null ならプレーン送信（plain は Rust が HTML から生成） */
  body_html: string | null;
  /** 添付ファイルの絶対パス。Rust がバイト列を読み込む */
  attachments: string[];
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
  /** 集約表示（サブツリー展開）でのみ埋まる。選択ノード自身を除く所属案件の集合 */
  projects: ThreadProjectRef[];
}

/** 集約表示でスレッドに付ける「どの案件のメールか」の注釈 */
export interface ThreadProjectRef {
  project_id: string;
  /** 選択ノードからの相対パス（例: "埼玉 > 音響"） */
  display_path: string;
}

/** 一括操作（bulk_delete_mails 等）の結果。1件の失敗で残りは止めない */
export interface BulkResult {
  succeeded: string[];
  /** [mail_id, エラーメッセージ] の組 */
  failed: [string, string][];
}

/** backfill_account の結果 */
export interface BackfillOutcome {
  /** 今回取り込んだ件数 */
  fetched: number;
  /** これ以上サーバーに古いメールが無いか */
  exhausted: boolean;
}
