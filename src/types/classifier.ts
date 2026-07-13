export interface ClassifyResponse {
  mail_id: string;
  action: "assign" | "create" | "unclassified";
  project_id?: string;
  project_name?: string;
  description?: string;
  confidence: number;
  reason: string;
}

/**
 * classify_batch の戻り値（Rust の ClassifyBatchOutcome）。
 * 1 invoke で「次の停止点（create 提案）or 完了/中断」まで進んだ結果。
 * done は処理済み件数、total はバッチ開始時のキュー長（再開しても不変）。
 */
export type ClassifyBatchOutcome =
  | { status: "completed"; done: number; total: number }
  | { status: "paused"; proposal: ClassifyResponse; done: number; total: number }
  | { status: "cancelled"; done: number; total: number }
  | { status: "already_running" };

/** classify-progress イベントの payload */
export interface ClassifyProgressEvent {
  account_id: string;
  current: number;
  total: number;
}
