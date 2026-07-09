use crate::classifier::TextGenerator;
use crate::error::AppError;
use crate::models::directory::ProjectFileEntry;

pub const DIGEST_SYSTEM_PROMPT: &str = "\
あなたは舞台制作の案件アシスタントです。案件フォルダのファイル一覧とテキスト資料から、
この案件の要約を Markdown の箇条書きで出力してください。

出力形式（この形式のみ、前置き・後置きなし）:
- 公演: <公演名・演目>
- 会場: <会場名とキーワード>
- 関係する組織・人: <資料から読み取れる関係先>
- キーワード: <メール分類の手がかりになる語>
- 主なファイル: <代表的なファイル名 5件まで>

読み取れない項目は行ごと省略する。推測で埋めない。全体で400字以内。";

pub fn build_digest_input(
    project_name: &str,
    files: &[ProjectFileEntry],
    texts: &[(String, String)],
) -> String {
    let mut input = format!("## 案件名\n{}\n\n## ファイル一覧\n", project_name);
    for f in files {
        input.push_str(&format!("- {}\n", f.relative_path));
    }
    if !texts.is_empty() {
        input.push_str("\n## テキスト資料の内容\n");
        for (path, text) in texts {
            input.push_str(&format!("### {}\n{}\n\n", path, text));
        }
    }
    input
}

pub async fn generate_digest(
    generator: &dyn TextGenerator,
    input: &str,
) -> Result<String, AppError> {
    generator.generate_text(DIGEST_SYSTEM_PROMPT, input).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classifier::TextGenerator;
    use crate::error::AppError;
    use crate::models::directory::ProjectFileEntry;
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

    fn entry(path: &str) -> ProjectFileEntry {
        ProjectFileEntry {
            relative_path: path.to_string(),
            size_bytes: 10,
            mtime: "2026-07-09T00:00:00Z".to_string(),
            content_hash: None,
            content_kind: "text".to_string(),
            extract_status: "ok".to_string(),
        }
    }

    #[test]
    fn test_build_digest_input_contains_files_and_texts() {
        let files = vec![entry("図面/平面図.pdf"), entry("香盤表.md")];
        let texts = vec![("香盤表.md".to_string(), "第1幕 くるみ割り".to_string())];
        let input = build_digest_input("〇〇ホール 春公演", &files, &texts);

        assert!(input.contains("〇〇ホール 春公演"));
        assert!(input.contains("図面/平面図.pdf"));
        assert!(input.contains("第1幕 くるみ割り"));
    }

    #[tokio::test]
    async fn test_generate_digest_returns_llm_output() {
        let generator = MockGenerator {
            response: "- 公演: くるみ割り人形\n- 会場: 〇〇ホール".to_string(),
        };
        let digest = generate_digest(&generator, "input").await.unwrap();
        assert!(digest.contains("くるみ割り人形"));
    }
}
