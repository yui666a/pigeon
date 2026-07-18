export interface Project {
  id: string;
  account_id: string;
  name: string;
  description: string | null;
  color: string | null;
  is_archived: boolean;
  parent_id: string | null;
  created_at: string;
  updated_at: string;
}

/** get_project_delete_impact の結果。削除確認ダイアログ用（サブツリーの案件数・メール件数） */
export interface DeleteImpact {
  projects: number;
  mails: number;
}

/** get_effective_context の1エントリ。祖先パス（ルート→自ノード）に沿った加算的コンテキスト */
export interface EffectiveContextEntry {
  project_id: string;
  project_name: string;
  is_self: boolean;
  directory_path: string | null;
  context: string | null;
}
