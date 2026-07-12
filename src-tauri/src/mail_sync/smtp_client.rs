//! SMTP送信 (lettre)。
//! メッセージ構築は純関数として切り出し、実送信 (`send`) と分離してテスト可能にする。

use std::time::Duration;

use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, Message, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

use crate::error::AppError;
use crate::models::account::AuthType;

/// 添付ファイル合計サイズの上限（25MB, Gmail 準拠）
pub const MAX_TOTAL_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;

/// 送信する添付ファイル。パスではなく読み込み済みのバイト列を持つ純データ
#[derive(Debug, Clone)]
pub struct OutgoingAttachment {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

/// 送信メッセージの構築入力。lettre型に依存しない純データ
#[derive(Debug, Clone)]
pub struct OutgoingMail {
    pub from_name: String,
    pub from_email: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    /// リッチ本文の HTML。Some なら multipart/alternative で送る。
    /// None ならプレーンのみ（後方互換）
    pub body_html: Option<String>,
    /// 添付ファイル（読み込み済み）。空なら添付なし
    pub attachments: Vec<OutgoingAttachment>,
    /// `<uuid@pigeon.local>` 形式。ローカルDBに保存する message_id と一致させる
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

/// ローカルDBと送信メッセージで共有する Message-ID を生成する
pub fn generate_message_id() -> String {
    format!("<{}@pigeon.local>", uuid::Uuid::new_v4())
}

/// 返信時の References ヘッダーを構築する（RFC 5322）。
/// 元メールの References に元メールの Message-ID を連結する。
/// 元メールに References が無ければ Message-ID 単独。既に含まれていれば重複させない
pub fn build_references(orig_references: Option<&str>, orig_message_id: &str) -> String {
    match orig_references {
        Some(refs) if !refs.trim().is_empty() => {
            let refs = refs.trim();
            if refs.contains(orig_message_id) {
                refs.to_string()
            } else {
                format!("{} {}", refs, orig_message_id)
            }
        }
        _ => orig_message_id.to_string(),
    }
}

/// TipTap が生成した HTML から plain text フォールバックを生成する。
/// 送信対象は自前の TipTap 出力に限定されるため、厳密な HTML パーサは使わず
/// タグ境界ベースで走査する（設計書 2026-07-13-rich-compose-design.md）。
pub fn html_to_plain(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let bytes = html.as_bytes();
    // '<' '>' は ASCII なのでバイト添字はUTF-8境界と一致する。テキスト区間は
    // バイト単位で切らず &str スライスとして丸ごとコピーし、マルチバイトを壊さない
    let mut i = 0;
    // 直前のタグ終端以降、まだ出力していないテキスト区間の開始位置
    let mut text_start = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            // タグ終端 '>' を探す
            let Some(rel) = html[i..].find('>') else {
                // 閉じない '<' 以降はすべてテキスト。ループ後にまとめて flush する
                break;
            };
            // '<' の手前までのテキスト区間をスライスコピー
            out.push_str(&html[text_start..i]);

            let tag = html[i + 1..i + rel].trim();
            let tag_lower = tag.to_ascii_lowercase();
            let name = tag_lower
                .trim_start_matches('/')
                .split(|c: char| c.is_whitespace() || c == '/')
                .next()
                .unwrap_or("");
            if name == "br" {
                out.push('\n');
            } else if tag_lower.starts_with('/')
                && matches!(
                    name,
                    "p" | "div" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol"
                )
            {
                out.push('\n');
            } else if name == "li" {
                // 開始タグの li は行頭のリストマーカー
                out.push_str("- ");
            }
            i += rel + 1;
            text_start = i;
        } else {
            i += 1;
        }
    }
    // 末尾に残ったテキスト区間（閉じない '<' 以降を含む）を flush
    out.push_str(&html[text_start..]);

    let decoded = decode_html_entities(&out);
    normalize_blank_lines(&decoded)
}

/// 最小限の HTML エンティティをデコードする
fn decode_html_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        // &amp; は最後（他のエンティティに含まれる & を誤変換しないため）
        .replace("&amp;", "&")
}

/// 連続する空行を最大2行（＝空行1つ）に圧縮し、各行末の空白と全体の前後空白を除去する
fn normalize_blank_lines(s: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    let mut blank_run = 0;
    for line in s.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                lines.push("");
            }
        } else {
            blank_run = 0;
            lines.push(trimmed);
        }
    }
    lines.join("\n").trim().to_string()
}

/// ファイル名の拡張子から Content-Type を素朴に推定する（不明は octet-stream）
pub fn guess_content_type(filename: &str) -> String {
    let ext = filename
        .rsplit('.')
        .next()
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let ct = match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "txt" | "log" | "md" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "xml" => "application/xml",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "doc" => "application/msword",
        "docx" => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        }
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => {
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        }
        _ => "application/octet-stream",
    };
    ct.to_string()
}

/// 添付ファイル合計サイズが上限以内か検証する。超過時は Validation エラー
pub fn validate_attachment_size(attachments: &[OutgoingAttachment]) -> Result<(), AppError> {
    let total: usize = attachments.iter().map(|a| a.data.len()).sum();
    if total > MAX_TOTAL_ATTACHMENT_BYTES {
        return Err(AppError::Validation(format!(
            "添付ファイルの合計サイズが上限({}MB)を超えています: {:.1}MB",
            MAX_TOTAL_ATTACHMENT_BYTES / (1024 * 1024),
            total as f64 / (1024.0 * 1024.0),
        )));
    }
    Ok(())
}

/// メールアドレス文字列を lettre Mailbox にパースする（`Name <a@b>` 形式も可）
fn parse_mailbox(addr: &str) -> Result<Mailbox, AppError> {
    addr.trim()
        .parse::<Mailbox>()
        .map_err(|e| AppError::Validation(format!("不正なメールアドレス: {} ({})", addr, e)))
}

/// OutgoingMail から lettre Message を構築する。
/// To が空、またはアドレスが不正な場合は Validation エラー
pub fn build_message(mail: &OutgoingMail) -> Result<Message, AppError> {
    if mail.to.iter().all(|a| a.trim().is_empty()) {
        return Err(AppError::Validation("宛先(To)が指定されていません".into()));
    }

    let from_addr = mail.from_email.parse::<lettre::Address>().map_err(|e| {
        AppError::Validation(format!("不正な送信元アドレス: {} ({})", mail.from_email, e))
    })?;
    let from_name = if mail.from_name.trim().is_empty() {
        None
    } else {
        Some(mail.from_name.trim().to_string())
    };

    let mut builder = Message::builder()
        .from(Mailbox::new(from_name, from_addr))
        .subject(mail.subject.clone())
        .message_id(Some(mail.message_id.clone()))
        .date_now();

    for to in mail.to.iter().filter(|a| !a.trim().is_empty()) {
        builder = builder.to(parse_mailbox(to)?);
    }
    for cc in mail.cc.iter().filter(|a| !a.trim().is_empty()) {
        builder = builder.cc(parse_mailbox(cc)?);
    }
    for bcc in mail.bcc.iter().filter(|a| !a.trim().is_empty()) {
        builder = builder.bcc(parse_mailbox(bcc)?);
    }
    if let Some(irt) = &mail.in_reply_to {
        builder = builder.in_reply_to(irt.clone());
    }
    if let Some(refs) = &mail.references {
        builder = builder.references(refs.clone());
    }

    validate_attachment_size(&mail.attachments)?;

    // 本文部（プレーンのみ or alternative）を組み立てる
    let body_part = build_body_part(mail);

    let build_err = |e: lettre::error::Error| AppError::Smtp(format!("メッセージ構築に失敗: {}", e));

    if mail.attachments.is_empty() {
        // 添付なし。alternative かプレーン singlepart をそのまま本文に
        match body_part {
            BodyPart::Plain(text) => builder
                .header(ContentType::TEXT_PLAIN)
                .body(text)
                .map_err(build_err),
            BodyPart::Alternative(mp) => builder.multipart(mp).map_err(build_err),
        }
    } else {
        // 添付あり。multipart/mixed に本文 + 各添付を並べる
        let mut mixed = MultiPart::mixed().build();
        mixed = match body_part {
            BodyPart::Plain(text) => mixed.singlepart(SinglePart::plain(text)),
            BodyPart::Alternative(mp) => mixed.multipart(mp),
        };
        for att in &mail.attachments {
            let content_type = att
                .content_type
                .parse::<ContentType>()
                .unwrap_or(ContentType::TEXT_PLAIN);
            mixed = mixed.singlepart(
                Attachment::new(att.filename.clone()).body(att.data.clone(), content_type),
            );
        }
        builder.multipart(mixed).map_err(build_err)
    }
}

/// 本文部の中間表現。プレーン単体か、plain+html の alternative
enum BodyPart {
    Plain(String),
    Alternative(MultiPart),
}

/// リッチ有無に応じて本文部を組み立てる。
/// リッチ時の plain フォールバックは HTML から生成する
fn build_body_part(mail: &OutgoingMail) -> BodyPart {
    match &mail.body_html {
        Some(html) => {
            let plain = html_to_plain(html);
            BodyPart::Alternative(MultiPart::alternative_plain_html(plain, html.clone()))
        }
        None => BodyPart::Plain(mail.body_text.clone()),
    }
}

/// SMTP送信。ポート465はImplicit TLS、それ以外はSTARTTLS。
/// 認証は AuthType に応じて PLAIN/LOGIN または XOAUTH2
pub async fn send(
    host: &str,
    port: u16,
    auth_type: &AuthType,
    username: &str,
    credential: &str,
    message: Message,
) -> Result<(), AppError> {
    let mechanisms = match auth_type {
        AuthType::Plain => vec![Mechanism::Plain, Mechanism::Login],
        AuthType::Oauth2 => vec![Mechanism::Xoauth2],
    };

    let builder = if port == 465 {
        AsyncSmtpTransport::<Tokio1Executor>::relay(host)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
    }
    .map_err(|e| AppError::Smtp(format!("SMTP接続設定に失敗: {}", e)))?;

    let transport = builder
        .port(port)
        .credentials(Credentials::new(
            username.to_string(),
            credential.to_string(),
        ))
        .authentication(mechanisms)
        .timeout(Some(Duration::from_secs(30)))
        .build();

    tokio::time::timeout(Duration::from_secs(30), transport.send(message))
        .await
        .map_err(|_| AppError::Smtp("SMTP送信がタイムアウトしました (30s)".into()))?
        .map_err(|e| AppError::Smtp(format!("SMTP送信に失敗: {}", e)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_mail() -> OutgoingMail {
        OutgoingMail {
            from_name: "Hiroshi".into(),
            from_email: "me@example.com".into(),
            to: vec!["tanaka@example.com".into()],
            cc: vec![],
            bcc: vec![],
            subject: "テスト件名".into(),
            body_text: "こんにちは".into(),
            body_html: None,
            attachments: vec![],
            message_id: "<abc-123@pigeon.local>".into(),
            in_reply_to: None,
            references: None,
        }
    }

    fn attachment(filename: &str, size: usize) -> OutgoingAttachment {
        OutgoingAttachment {
            filename: filename.into(),
            content_type: guess_content_type(filename),
            data: vec![0u8; size],
        }
    }

    #[test]
    fn test_generate_message_id_format() {
        let id = generate_message_id();
        assert!(id.starts_with('<'));
        assert!(id.ends_with("@pigeon.local>"));
        // 2回呼ぶと異なるIDになる
        assert_ne!(id, generate_message_id());
    }

    #[test]
    fn test_build_references_no_orig_references() {
        assert_eq!(build_references(None, "<a@ex.com>"), "<a@ex.com>");
        assert_eq!(build_references(Some("  "), "<a@ex.com>"), "<a@ex.com>");
    }

    #[test]
    fn test_build_references_appends_message_id() {
        assert_eq!(
            build_references(Some("<root@ex.com> <mid@ex.com>"), "<a@ex.com>"),
            "<root@ex.com> <mid@ex.com> <a@ex.com>"
        );
    }

    #[test]
    fn test_build_references_no_duplicate() {
        assert_eq!(
            build_references(Some("<root@ex.com> <a@ex.com>"), "<a@ex.com>"),
            "<root@ex.com> <a@ex.com>"
        );
    }

    #[test]
    fn test_build_message_basic_headers() {
        let msg = build_message(&base_mail()).unwrap();
        let raw = String::from_utf8(msg.formatted()).unwrap();
        assert!(
            raw.contains("From: \"Hiroshi\" <me@example.com>")
                || raw.contains("From: Hiroshi <me@example.com>")
        );
        assert!(raw.contains("To: <tanaka@example.com>") || raw.contains("To: tanaka@example.com"));
        assert!(raw.contains("Message-ID: <abc-123@pigeon.local>"));
        assert!(raw.contains("Subject:"));
    }

    #[test]
    fn test_build_message_reply_headers() {
        let mut mail = base_mail();
        mail.in_reply_to = Some("<orig@ex.com>".into());
        mail.references = Some("<root@ex.com> <orig@ex.com>".into());
        let msg = build_message(&mail).unwrap();
        let raw = String::from_utf8(msg.formatted()).unwrap();
        assert!(raw.contains("In-Reply-To: <orig@ex.com>"));
        assert!(raw.contains("References: <root@ex.com> <orig@ex.com>"));
    }

    #[test]
    fn test_build_message_multiple_recipients() {
        let mut mail = base_mail();
        mail.to = vec!["a@ex.com".into(), "b@ex.com".into()];
        mail.cc = vec!["c@ex.com".into()];
        mail.bcc = vec!["d@ex.com".into()];
        let msg = build_message(&mail).unwrap();
        let raw = String::from_utf8(msg.formatted()).unwrap();
        assert!(raw.contains("a@ex.com"));
        assert!(raw.contains("b@ex.com"));
        assert!(raw.contains("Cc:"));
        // Bcc はヘッダーに含まれない（envelope のみ）が、エラーにはならない
    }

    #[test]
    fn test_build_message_empty_to_rejected() {
        let mut mail = base_mail();
        mail.to = vec![];
        assert!(matches!(build_message(&mail), Err(AppError::Validation(_))));
        let mut mail2 = base_mail();
        mail2.to = vec!["  ".into()];
        assert!(matches!(
            build_message(&mail2),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn test_build_message_invalid_address_rejected() {
        let mut mail = base_mail();
        mail.to = vec!["not-an-address".into()];
        assert!(matches!(build_message(&mail), Err(AppError::Validation(_))));
    }

    #[test]
    fn test_build_message_from_name_empty_ok() {
        let mut mail = base_mail();
        mail.from_name = "".into();
        let msg = build_message(&mail).unwrap();
        let raw = String::from_utf8(msg.formatted()).unwrap();
        assert!(raw.contains("me@example.com"));
    }

    #[test]
    fn test_html_to_plain_breaks_and_blocks() {
        assert_eq!(html_to_plain("<p>Hello</p><p>World</p>"), "Hello\nWorld");
        assert_eq!(html_to_plain("line1<br>line2"), "line1\nline2");
        assert_eq!(html_to_plain("<div>a</div><div>b</div>"), "a\nb");
    }

    #[test]
    fn test_html_to_plain_list_items() {
        assert_eq!(
            html_to_plain("<ul><li>one</li><li>two</li></ul>"),
            "- one\n- two"
        );
    }

    #[test]
    fn test_html_to_plain_strips_inline_tags_and_entities() {
        assert_eq!(
            html_to_plain("<p><strong>bold</strong> &amp; <em>italic</em></p>"),
            "bold & italic"
        );
        assert_eq!(html_to_plain("a&lt;b&gt;c&nbsp;d"), "a<b>c d");
    }

    #[test]
    fn test_html_to_plain_preserves_multibyte_utf8() {
        // 日本語（マルチバイトUTF-8）がバイト分割で文字化けしないこと
        assert_eq!(html_to_plain("<p>こんにちは</p>"), "こんにちは");
        assert_eq!(
            html_to_plain("<p>日本語の<strong>本文</strong>です</p>"),
            "日本語の本文です"
        );
        // 絵文字（4バイト）も壊れないこと
        assert_eq!(html_to_plain("<p> hi 🕊️ bye</p>"), "hi 🕊️ bye");
    }

    #[test]
    fn test_html_to_plain_unclosed_bracket_kept_as_text() {
        // 閉じない '<' 以降は文字として残す（マルチバイトも壊さない）
        assert_eq!(html_to_plain("a < b の話"), "a < b の話");
        assert_eq!(html_to_plain("<p>x</p>y < z"), "x\ny < z");
    }

    #[test]
    fn test_html_to_plain_collapses_blank_lines() {
        // 複数の空ブロックが連続しても空行は1つに圧縮され前後はtrimされる
        assert_eq!(
            html_to_plain("<p>a</p><p></p><p></p><p>b</p>"),
            "a\n\nb"
        );
    }

    #[test]
    fn test_guess_content_type() {
        assert_eq!(guess_content_type("doc.pdf"), "application/pdf");
        assert_eq!(guess_content_type("img.PNG"), "image/png");
        assert_eq!(guess_content_type("photo.jpeg"), "image/jpeg");
        assert_eq!(guess_content_type("noext"), "application/octet-stream");
        assert_eq!(guess_content_type("archive.zip"), "application/zip");
    }

    #[test]
    fn test_validate_attachment_size_within_limit() {
        let atts = vec![attachment("a.bin", 1024), attachment("b.bin", 2048)];
        assert!(validate_attachment_size(&atts).is_ok());
    }

    #[test]
    fn test_validate_attachment_size_over_limit() {
        let atts = vec![attachment("big.bin", MAX_TOTAL_ATTACHMENT_BYTES + 1)];
        assert!(matches!(
            validate_attachment_size(&atts),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn test_build_message_plain_only_is_singlepart() {
        // body_html なし・添付なしは従来どおり text/plain singlepart
        let msg = build_message(&base_mail()).unwrap();
        let raw = String::from_utf8(msg.formatted()).unwrap();
        assert!(raw.contains("Content-Type: text/plain"));
        assert!(!raw.contains("multipart/"));
    }

    #[test]
    fn test_build_message_rich_is_multipart_alternative() {
        let mut mail = base_mail();
        mail.body_html = Some("<p>Hello <strong>world</strong></p>".into());
        let msg = build_message(&mail).unwrap();
        let raw = String::from_utf8(msg.formatted()).unwrap();
        assert!(raw.contains("multipart/alternative"));
        assert!(raw.contains("text/plain"));
        assert!(raw.contains("text/html"));
    }

    #[test]
    fn test_build_message_with_attachment_is_multipart_mixed() {
        let mut mail = base_mail();
        let mut ascii = attachment("report.pdf", 8);
        ascii.data = b"%PDF-1.4".to_vec();
        mail.attachments = vec![ascii];
        let msg = build_message(&mail).unwrap();
        let raw = String::from_utf8(msg.formatted()).unwrap();
        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("application/pdf"));
        assert!(raw.contains("report.pdf"));
    }

    #[test]
    fn test_build_message_rich_with_attachment_nests_alternative_in_mixed() {
        let mut mail = base_mail();
        mail.body_html = Some("<p>hi</p>".into());
        mail.attachments = vec![attachment("a.png", 4)];
        let msg = build_message(&mail).unwrap();
        let raw = String::from_utf8(msg.formatted()).unwrap();
        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("multipart/alternative"));
        assert!(raw.contains("image/png"));
    }

    #[test]
    fn test_build_message_rejects_oversized_attachment() {
        let mut mail = base_mail();
        mail.attachments = vec![attachment("big.bin", MAX_TOTAL_ATTACHMENT_BYTES + 1)];
        assert!(matches!(
            build_message(&mail),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn test_body_is_included() {
        let msg = build_message(&base_mail()).unwrap();
        let raw = String::from_utf8(msg.formatted()).unwrap();
        // UTF-8本文はbase64またはquoted-printableでエンコードされる場合があるため、
        // ASCII本文で内容が残ることを別途確認する
        let mut ascii = base_mail();
        ascii.body_text = "hello world".into();
        let raw_ascii = String::from_utf8(build_message(&ascii).unwrap().formatted()).unwrap();
        assert!(raw_ascii.contains("hello world"));
        assert!(!raw.is_empty());
    }
}
