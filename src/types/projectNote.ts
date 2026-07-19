/** 案件ノート（Rust `ProjectNote` の TS 側ミラー）。フィールド名は snake_case のまま保持する。 */
export interface ProjectNote {
  project_id: string;
  user_md: string;
  ai_md: string | null;
  ai_edited: boolean;
  ai_generated_at: string | null;
  updated_at: string | null;
}

/** AI要約の世代管理履歴（Rust `AiHistoryEntry` の TS 側ミラー）。 */
export interface AiHistoryEntry {
  id: string;
  project_id: string;
  ai_md: string;
  replaced_at: string;
}

/** AI要約生成コマンドの戻り値（Rust `GenerateNoteOutcome` の TS 側ミラー）。 */
export interface GenerateNoteOutcome {
  ai_md: string;
  dropped_mails: number;
}
