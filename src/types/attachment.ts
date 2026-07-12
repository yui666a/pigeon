export interface Attachment {
  id: string;
  mail_id: string;
  filename: string;
  mime_type: string;
  size: number | null;
  file_path: string | null;
}
