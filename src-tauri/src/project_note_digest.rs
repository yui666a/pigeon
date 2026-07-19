use crate::classifier::TextGenerator;
use crate::error::AppError;
use crate::models::mail::Mail;

/// AI要約に使うメール件数の上限。超過分は切り捨て、件数を呼び出し元へ返す。
pub const MAX_MAILS: usize = 50;
/// 1通あたりの本文送信上限（ADR-0002 のクラウド送信境界と同一）。
pub const BODY_HEAD_CHARS: usize = 1000;

pub const MAIL_DIGEST_SYSTEM_PROMPT: &str = "\
あなたは舞台制作の案件アシスタントです。案件に属するメールのやり取りから、
この案件の要約を Markdown の箇条書きで出力してください。

出力形式（この形式のみ、前置き・後置きなし）:
- 公演: <公演名・演目>
- 会場: <会場名とキーワード>
- 関係する組織・人: <メールから読み取れる関係先>
- キーワード: <メール分類の手がかりになる語>
- 主なやり取り: <論点・決定事項を3件まで>

読み取れない項目は行ごと省略する。推測で埋めない。全体で400字以内。";

/// メール群から LLM への入力を組み立てる。
/// 戻り値は (入力文字列, 切り捨てたメール件数)。
/// 送信するのは件名・送信者・本文冒頭 BODY_HEAD_CHARS 文字のみ（ADR-0002）。
pub fn build_mail_digest_input(project_name: &str, mails: &[Mail]) -> (String, usize) {
    let dropped = mails.len().saturating_sub(MAX_MAILS);
    let used = if mails.len() > MAX_MAILS {
        &mails[..MAX_MAILS]
    } else {
        mails
    };

    let mut input = format!("## 案件名\n{}\n\n", project_name);
    for (i, m) in used.iter().enumerate() {
        input.push_str(&format!("### メール{}\n", i + 1));
        input.push_str(&format!("- 件名: {}\n", m.subject));
        input.push_str(&format!("- 送信者: {}\n", m.from_addr));
        if let Some(body) = &m.body_text {
            let head: String = body.chars().take(BODY_HEAD_CHARS).collect();
            input.push_str(&format!("- 本文冒頭:\n{}\n", head));
        }
        input.push('\n');
    }
    (input, dropped)
}

pub async fn generate_mail_digest(
    generator: &dyn TextGenerator,
    input: &str,
) -> Result<String, AppError> {
    generator
        .generate_text(MAIL_DIGEST_SYSTEM_PROMPT, input)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::make_mail;
    use async_trait::async_trait;

    struct MockGenerator {
        response: String,
    }

    #[async_trait]
    impl TextGenerator for MockGenerator {
        async fn generate_text(&self, _system: &str, _user: &str) -> Result<String, AppError> {
            Ok(self.response.clone())
        }
    }

    fn mail_with_body(id: &str, subject: &str, from: &str, body: &str) -> Mail {
        let mut m = make_mail(id, &format!("<{}@e>", id), subject, "2026-07-19T10:00:00Z");
        m.from_addr = from.to_string();
        m.body_text = Some(body.to_string());
        m
    }

    #[test]
    fn test_build_input_includes_subject_and_from() {
        let mails = vec![mail_with_body(
            "m1",
            "搬入の件",
            "a@example.com",
            "本文です",
        )];
        let (input, dropped) = build_mail_digest_input("春公演", &mails);
        assert!(input.contains("春公演"));
        assert!(input.contains("搬入の件"));
        assert!(input.contains("a@example.com"));
        assert!(input.contains("本文です"));
        assert_eq!(dropped, 0);
    }

    #[test]
    fn test_build_input_truncates_body_at_boundary() {
        let long_body = "あ".repeat(BODY_HEAD_CHARS + 500);
        let mails = vec![mail_with_body("m1", "件名", "a@example.com", &long_body)];
        let (input, _) = build_mail_digest_input("P", &mails);
        let body_chars = input.matches('あ').count();
        assert_eq!(
            body_chars, BODY_HEAD_CHARS,
            "本文は冒頭1000文字までしか含めない（ADR-0002の送信境界）"
        );
    }

    #[test]
    fn test_build_input_caps_mail_count_and_reports_dropped() {
        let mails: Vec<Mail> = (0..(MAX_MAILS + 7))
            .map(|i| mail_with_body(&format!("m{}", i), "件名", "a@example.com", "本文"))
            .collect();
        let (input, dropped) = build_mail_digest_input("P", &mails);
        assert_eq!(dropped, 7, "超過分の件数を返す（サイレント切り捨て禁止）");
        assert_eq!(input.matches("### メール").count(), MAX_MAILS);
    }

    #[test]
    fn test_build_input_handles_missing_body() {
        let mut m = make_mail("m1", "<m1@e>", "件名のみ", "2026-07-19T10:00:00Z");
        m.body_text = None;
        let (input, _) = build_mail_digest_input("P", &[m]);
        assert!(input.contains("件名のみ"), "本文が無くても件名は含まれる");
    }

    #[tokio::test]
    async fn test_generate_mail_digest_returns_llm_output() {
        let gen = MockGenerator {
            response: "- 公演: 春公演\n- 会場: 〇〇ホール".to_string(),
        };
        let out = generate_mail_digest(&gen, "入力").await.unwrap();
        assert!(out.contains("春公演"));
    }
}
