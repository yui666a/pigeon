export interface Attachment {
  id: string;
  mail_id: string;
  filename: string;
  mime_type: string;
  size: number | null;
  file_path: string | null;
  content_id: string | null;
}

/** 本文中の `<img src="cid:...">` に対応する画像データ */
export interface InlineImage {
  content_id: string;
  data_uri: string;
}
