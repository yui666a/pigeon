export interface ClassifyResponse {
  mail_id: string;
  action: "assign" | "create" | "unclassified";
  project_id?: string;
  project_name?: string;
  description?: string;
  /** action="create" のとき、既存案件配下に子案件として作成する提案なら設定される */
  parent_project_id?: string;
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
  /**
   * このステップで案件へ確定割り当てされたメールの ID（あれば）。
   * バッチ分類中に、確定したメールを未分類一覧から即座に消すために使う。
   * 確信度不足で未分類に留まった場合や Create 提案の場合は null。
   */
  assigned_mail_id: string | null;
}

/** suggest_project_from_mails の戻り値（Rust の ProjectSuggestion）。 */
export interface ProjectSuggestion {
  name: string;
  description: string;
}
