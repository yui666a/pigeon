export interface MapPoint {
  x: number;
  y: number;
  mail_id: string;
  subject: string;
  project_id: string | null;
  project_name: string | null;
  project_color: string | null;
}

export interface MailPreview {
  mail_id: string;
  subject: string;
  from_addr: string;
  date: string;
  body_excerpt: string;
}

/** 案件パネル（ドロップ先）用の軽量な案件情報。Rust 側 MapProject と対 */
export interface MapProject {
  id: string;
  name: string;
  color: string | null;
}
