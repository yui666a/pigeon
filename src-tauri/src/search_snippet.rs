//! 原文ベースのスニペット生成。
//! fts_mails には正規化済みテキストを格納しているため FTS5 の snippet() は
//! カタカナ・小文字化された文字列を返してしまう。ここでは正規化オフセット
//! 対応表を使って原文からスニペットを切り出す。

use crate::search_normalize::{normalize_for_search, normalize_with_offsets};

/// マッチ位置の前後に残す最大文字数
const CONTEXT_CHARS: usize = 30;

pub fn make_snippet(original: &str, query: &str) -> Option<String> {
    let norm_query = normalize_for_search(query);
    if norm_query.is_empty() {
        return None;
    }
    let norm = normalize_with_offsets(original);
    // find はバイト単位の部分文字列検索。返るのはバイト位置なので、
    // offsets 参照の前に必ず char 位置へ換算する（直下の chars().count()）
    let byte_start = norm.text.find(&norm_query)?;

    // 正規化テキスト内の char 位置に変換し、オフセット対応表で原文バイト位置へ
    let char_start = norm.text[..byte_start].chars().count();
    let char_end = char_start + norm_query.chars().count();
    let orig_start = norm.offsets[char_start];
    let orig_end = if char_end < norm.offsets.len() {
        norm.offsets[char_end]
    } else {
        original.len()
    };
    // NFKC 展開（㈱→"(株)" 等）でマッチ終端が始端と同じ原文 char に
    // 対応した場合でも、最低 1 文字はハイライトする
    let orig_end = orig_end.max(next_char_boundary(original, orig_start));

    let before = &original[..orig_start];
    let matched = &original[orig_start..orig_end];
    let after = &original[orig_end..];

    let prefix: String = {
        let chars: Vec<char> = before.chars().collect();
        let start = chars.len().saturating_sub(CONTEXT_CHARS);
        chars[start..].iter().collect()
    };
    let suffix: String = after.chars().take(CONTEXT_CHARS).collect();
    let head_truncated = before.chars().count() > CONTEXT_CHARS;
    let tail_truncated = after.chars().count() > CONTEXT_CHARS;

    let mut snippet = String::new();
    if head_truncated {
        snippet.push_str("...");
    }
    snippet.push_str(&prefix);
    snippet.push_str("<b>");
    snippet.push_str(matched);
    snippet.push_str("</b>");
    snippet.push_str(&suffix);
    if tail_truncated {
        snippet.push_str("...");
    }
    Some(snippet)
}

/// byte_pos の次の char 境界（byte_pos が原文末尾なら原文長）
fn next_char_boundary(s: &str, byte_pos: usize) -> usize {
    s[byte_pos..]
        .chars()
        .next()
        .map(|c| byte_pos + c.len_utf8())
        .unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match_is_highlighted() {
        assert_eq!(
            make_snippet("見積もりの件", "見積もり"),
            Some("<b>見積もり</b>の件".into())
        );
    }

    #[test]
    fn test_normalized_match_highlights_original_text() {
        // クエリ "sato" が原文の全角 "ＳＡＴＯ" にマッチし、原文表記のまま返る
        assert_eq!(
            make_snippet("ＳＡＴＯ商事より", "sato"),
            Some("<b>ＳＡＴＯ</b>商事より".into())
        );
    }

    #[test]
    fn test_hiragana_query_matches_katakana_text() {
        assert_eq!(
            make_snippet("サトーの端末", "さとー"),
            Some("<b>サトー</b>の端末".into())
        );
    }

    #[test]
    fn test_no_match_returns_none() {
        assert_eq!(make_snippet("こんにちは", "xyz"), None);
    }

    #[test]
    fn test_empty_query_returns_none() {
        assert_eq!(make_snippet("こんにちは", ""), None);
    }

    #[test]
    fn test_long_text_is_truncated_with_ellipsis() {
        let body = format!("{}KEYWORD{}", "あ".repeat(50), "い".repeat(50));
        let snip = make_snippet(&body, "keyword").unwrap();
        assert!(snip.starts_with("..."));
        assert!(snip.ends_with("..."));
        assert!(snip.contains("<b>KEYWORD</b>"));
        // 前後 30 文字ずつに切り詰められている
        assert!(snip.contains(&"あ".repeat(30)));
        assert!(!snip.contains(&"あ".repeat(31)));
    }

    #[test]
    fn test_short_text_no_ellipsis() {
        let snip = make_snippet("abc KEYWORD def", "keyword").unwrap();
        assert_eq!(snip, "abc <b>KEYWORD</b> def");
    }

    #[test]
    fn test_match_at_string_start() {
        assert_eq!(
            make_snippet("KEYWORD のあと", "keyword"),
            Some("<b>KEYWORD</b> のあと".into())
        );
    }

    #[test]
    fn test_match_at_string_end() {
        assert_eq!(
            make_snippet("まえ KEYWORD", "keyword"),
            Some("まえ <b>KEYWORD</b>".into())
        );
    }

    #[test]
    fn test_emoji_in_original_text() {
        // 4 バイト文字（絵文字）を含む原文でもバイト境界でパニックしない
        assert_eq!(
            make_snippet("🎭 照明の件 🎭", "照明"),
            Some("🎭 <b>照明</b>の件 🎭".into())
        );
    }
}
