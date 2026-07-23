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
