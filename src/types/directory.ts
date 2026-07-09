export interface ProjectDirectory {
  id: string;
  project_id: string;
  path: string;
  is_primary: boolean;
  status: "ok" | "missing" | "inaccessible" | "error";
  last_scanned_at: string | null;
  created_at: string;
}

export interface ProjectFile {
  id: string;
  directory_id: string;
  relative_path: string;
  size_bytes: number;
  mtime: string;
  content_hash: string | null;
  content_kind: "none" | "text" | "pdf" | "office" | "other";
  extract_status: "ok" | "skipped_too_large" | "unsupported" | "error";
  indexed_at: string;
}

export interface CloudRule {
  id: string;
  directory_id: string;
  scope: "directory" | "file";
  relative_path: string;
  allow: boolean;
}

export interface ProjectContext {
  project_id: string;
  cached_context: string | null;
  context_hash: string | null;
  inventory_hash: string | null;
  allow_cloud_context: boolean;
  generated_at: string | null;
}

export interface RescanOutcome {
  status: string;
  regenerated: boolean;
  file_count: number;
}
