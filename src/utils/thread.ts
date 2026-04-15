import type { Mail, Thread } from "../types/mail";

export function threadFromMail(mail: Mail): Thread {
  return {
    thread_id: mail.message_id || mail.id,
    subject: mail.subject,
    last_date: mail.date,
    mail_count: 1,
    from_addrs: [mail.from_addr],
    mails: [mail],
  };
}
