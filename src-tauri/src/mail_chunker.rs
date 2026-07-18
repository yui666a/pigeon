//! メール本文のチャンク化。埋め込み（ベクトル索引）の入力単位を作る。
//! - 返信引用（`>` 行・引用ヘッダ以降）は索引の重複を招くため除去する
//! - 分割は文字数ベース約800文字、段落境界（空行）優先、100文字オーバーラップ
//! - 各チャンクに件名をプレフィックスとして付与する（件名は強い検索シグナル）

/// 1チャンクの目標文字数（本文部分。プレフィックスは含まない）
pub const CHUNK_TARGET_CHARS: usize = 800;
/// 隣接チャンクとのオーバーラップ文字数
pub const CHUNK_OVERLAP_CHARS: usize = 100;
/// 段落境界を探して切り戻す最大文字数
const BREAK_LOOKBACK_CHARS: usize = 200;

pub fn remove_quoted_lines(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('>') {
            continue; // 行頭引用はスキップ
        }
        if is_quote_block_header(trimmed) {
            break; // 引用ブロックヘッダ以降は全て引用とみなし打ち切る
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// 引用ブロックの開始行か（保守的な判定。取りこぼしは検索ノイズになるだけで
/// 致命的でないため、誤除去しない側に倒す）
fn is_quote_block_header(line: &str) -> bool {
    line.starts_with("-----Original Message-----")
        || line.starts_with("----- Original Message -----")
        || (line.starts_with("On ") && line.trim_end().ends_with("wrote:"))
}

pub fn chunk_mail(subject: &str, body_text: Option<&str>) -> Vec<String> {
    let prefix = format!("件名: {}\n", subject);
    let body = body_text.map(remove_quoted_lines).unwrap_or_default();
    if body.trim().is_empty() {
        return vec![format!("件名: {}", subject)];
    }
    let chars: Vec<char> = body.chars().collect();
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let hard_end = (start + CHUNK_TARGET_CHARS).min(chars.len());
        let end = find_break(&chars, start, hard_end);
        let piece: String = chars[start..end].iter().collect();
        chunks.push(format!("{}{}", prefix, piece.trim()));
        if end >= chars.len() {
            break;
        }
        // オーバーラップ分戻る。ただし必ず前進させる（無限ループ防止）
        let next = end.saturating_sub(CHUNK_OVERLAP_CHARS);
        start = if next > start { next } else { end };
    }
    chunks
}

/// hard_end から最大 BREAK_LOOKBACK_CHARS だけ手前に空行（段落境界）を探す。
/// 見つからなければ hard_end で切る。
fn find_break(chars: &[char], start: usize, hard_end: usize) -> usize {
    if hard_end >= chars.len() {
        return chars.len();
    }
    let floor = hard_end.saturating_sub(BREAK_LOOKBACK_CHARS).max(start + 1);
    let mut i = hard_end;
    while i > floor {
        if chars[i - 1] == '\n' && i >= 2 && chars[i - 2] == '\n' {
            return i;
        }
        i -= 1;
    }
    hard_end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remove_gt_quoted_lines() {
        let body = "了解です。\n> 前回のメール\n> の引用\n以上です。";
        assert_eq!(remove_quoted_lines(body), "了解です。\n以上です。");
    }

    #[test]
    fn test_remove_original_message_block() {
        let body = "本文です。\n----- Original Message -----\nここから下は全部引用";
        assert_eq!(remove_quoted_lines(body), "本文です。");
    }

    #[test]
    fn test_remove_on_wrote_block() {
        let body = "返信本文\nOn Thu, Jul 17, 2026 sato wrote:\n引用引用";
        assert_eq!(remove_quoted_lines(body), "返信本文");
    }

    #[test]
    fn test_short_mail_is_single_chunk_with_subject_prefix() {
        let chunks = chunk_mail("照明の件", Some("仕込み図を送ります"));
        assert_eq!(chunks, vec!["件名: 照明の件\n仕込み図を送ります".to_string()]);
    }

    #[test]
    fn test_empty_body_yields_subject_only_chunk() {
        // 本文なしでも必ず1チャンク返す（「処理済み」マーカーを兼ねるため空Vecは不可）
        assert_eq!(chunk_mail("件名だけ", None), vec!["件名: 件名だけ".to_string()]);
        assert_eq!(chunk_mail("件名だけ", Some("")), vec!["件名: 件名だけ".to_string()]);
    }

    #[test]
    fn test_long_body_splits_with_overlap() {
        let body = "あ".repeat(2000);
        let chunks = chunk_mail("長文", Some(&body));
        assert!(chunks.len() >= 2, "2000文字は複数チャンクになる");
        for c in &chunks {
            assert!(c.starts_with("件名: 長文\n"), "全チャンクに件名プレフィックス");
            let body_part = c.trim_start_matches("件名: 長文\n");
            assert!(body_part.chars().count() <= CHUNK_TARGET_CHARS);
        }
        // オーバーラップ: 隣接チャンクは末尾/先頭を共有する
        let first_body = chunks[0].trim_start_matches("件名: 長文\n");
        let second_body = chunks[1].trim_start_matches("件名: 長文\n");
        let first_tail: String = first_body
            .chars()
            .skip(first_body.chars().count() - CHUNK_OVERLAP_CHARS)
            .collect();
        assert!(second_body.starts_with(&first_tail));
    }

    #[test]
    fn test_split_prefers_paragraph_break() {
        // 750文字目に空行 → 800で機械的に切らず段落境界で切る
        let body = format!("{}\n\n{}", "い".repeat(750), "う".repeat(500));
        let chunks = chunk_mail("s", Some(&body));
        let first_body = chunks[0].trim_start_matches("件名: s\n");
        assert_eq!(first_body, "い".repeat(750));
    }

    #[test]
    fn test_termination_on_pathological_input() {
        // オーバーラップで無限ループしないこと（進行保証）
        let body = "え".repeat(CHUNK_OVERLAP_CHARS + 1);
        let chunks = chunk_mail("s", Some(&body));
        assert_eq!(chunks.len(), 1);
    }
}
