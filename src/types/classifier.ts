export interface ClassifyResponse {
  mail_id: string;
  action: "assign" | "create" | "unclassified";
  project_id?: string;
  project_name?: string;
  description?: string;
  confidence: number;
  reason: string;
}

export interface ClassifyProgress {
  current: number;
  total: number;
  mail_id: string;
  result: ClassifyResponse;
}

export interface ClassifySummary {
  total: number;
  assigned: number;
  needs_review: number;
  unclassified: number;
}
