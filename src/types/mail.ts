export interface Mail {
  id: string;
  account_id: string;
  folder: string;
  message_id: string;
  in_reply_to: string | null;
  references: string | null;
  from_addr: string;
  to_addr: string;
  cc_addr: string | null;
  subject: string;
  body_text: string | null;
  body_html: string | null;
  date: string;
  has_attachments: boolean;
  raw_size: number | null;
  uid: number;
  flags: string | null;
  fetched_at: string;
}

export interface Thread {
  thread_id: string;
  subject: string;
  last_date: string;
  mail_count: number;
  from_addrs: string[];
  mails: Mail[];
}
