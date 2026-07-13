export interface ClassifyResponse {
  mail_id: string;
  action: "assign" | "create" | "unclassified";
  project_id?: string;
  project_name?: string;
  description?: string;
  confidence: number;
  reason: string;
}

/** get_unclassified_mails が返す未分類メールの参照（分類キュー用） */
export interface UnclassifiedMailRef {
  id: string;
}

export interface ClassifySummary {
  total: number;
  assigned: number;
  needs_review: number;
  unclassified: number;
}
