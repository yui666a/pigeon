//! 検索用テキスト正規化。
//! 索引時（fts_mails への書き込み）とクエリ時の両方に同じ正規化を適用することで、
//! 全角/半角・ひらがな/カタカナ・大文字/小文字の表記揺れを吸収する。
//! オフセット対応表は原文スニペット生成（search_snippet）に使う。

use unicode_normalization::UnicodeNormalization;

/// 正規化結果。`offsets[i]` は `text` の i 番目の char が
/// 原文の何バイト目の char に由来するかを表す。
pub struct NormalizedText {
    pub text: String,
    pub offsets: Vec<usize>,
}

pub fn normalize_with_offsets(input: &str) -> NormalizedText {
    let mut text = String::with_capacity(input.len());
    let mut offsets: Vec<usize> = Vec::new();
    // 1 文字ずつ NFKC → ひらがな→カタカナ → 小文字化 の順に適用し、
    // 生成された各 char に元 char のバイト位置を対応づける。
    // 文字列全体でなく 1 文字ずつ NFKC する理由はオフセット対応を保つため。
    // 文字またぎの合成（半角濁点カナ等）は push_normalized 内で処理する。
    for (byte_pos, ch) in input.char_indices() {
        for nfkc_ch in ch.nfkc() {
            let folded = hiragana_to_katakana(nfkc_ch);
            for lower in folded.to_lowercase() {
                push_normalized(&mut text, &mut offsets, lower, byte_pos);
            }
        }
    }
    NormalizedText { text, offsets }
}

pub fn normalize_for_search(input: &str) -> String {
    normalize_with_offsets(input).text
}

/// ひらがな→カタカナ（U+3041..U+3096, 繰り返し記号 U+309D/309E を +0x60 シフト）
fn hiragana_to_katakana(ch: char) -> char {
    match ch {
        'ぁ'..='ゖ' | 'ゝ' | 'ゞ' => char::from_u32(ch as u32 + 0x60).unwrap_or(ch),
        _ => ch,
    }
}

/// 正規化済み char を 1 つ追加する。結合濁点/半濁点（半角ｶﾞ等の NFKC 分解で
/// 現れる U+3099/U+309A）は直前の文字と NFC 合成して 1 文字に戻す。
fn push_normalized(text: &mut String, offsets: &mut Vec<usize>, ch: char, byte_pos: usize) {
    if matches!(ch, '\u{3099}' | '\u{309A}') {
        if let (Some(prev), Some(prev_off)) = (text.chars().last(), offsets.last().copied()) {
            if let Some(composed) = compose_pair(prev, ch) {
                text.pop();
                offsets.pop();
                text.push(composed);
                offsets.push(prev_off);
                return;
            }
        }
    }
    text.push(ch);
    offsets.push(byte_pos);
}

/// 2 文字を NFC 合成して 1 文字になれば返す（カ + U+3099 → ガ 等）
fn compose_pair(base: char, mark: char) -> Option<char> {
    let mut buf = String::with_capacity(8);
    buf.push(base);
    buf.push(mark);
    let mut it = buf.nfc();
    let composed = it.next()?;
    if it.next().is_none() {
        Some(composed)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii_lowercase() {
        assert_eq!(normalize_for_search("SATO"), "sato");
    }

    #[test]
    fn test_fullwidth_alnum_to_halfwidth() {
        assert_eq!(normalize_for_search("ＳＡＴＯ１２３"), "sato123");
    }

    #[test]
    fn test_hiragana_to_katakana() {
        assert_eq!(normalize_for_search("さとー"), "サトー");
    }

    #[test]
    fn test_halfwidth_katakana_to_fullwidth() {
        assert_eq!(normalize_for_search("ｻﾄｰ"), "サトー");
    }

    #[test]
    fn test_halfwidth_voiced_katakana_composes() {
        // 半角の濁点つきカナは NFKC で「カ + 結合濁点」に分解されるため、
        // 合成して 1 文字に戻すこと
        assert_eq!(normalize_for_search("ﾃﾞﾊﾞｲｽ"), "デバイス");
    }

    #[test]
    fn test_mixed_text() {
        assert_eq!(
            normalize_for_search("Ｐｒｉｎｔｅｒのみつもり"),
            "printerノミツモリ"
        );
    }

    #[test]
    fn test_nfkc_expansion() {
        // ㈱ は NFKC で 3 文字に展開される
        assert_eq!(normalize_for_search("㈱サトー"), "(株)サトー");
    }

    #[test]
    fn test_empty() {
        let n = normalize_with_offsets("");
        assert_eq!(n.text, "");
        assert!(n.offsets.is_empty());
    }

    #[test]
    fn test_offsets_map_to_original_bytes() {
        // 原文: "aＢか" = a(1byte) + Ｂ(3bytes) + か(3bytes)
        // 正規化: "abカ"
        let n = normalize_with_offsets("aＢか");
        assert_eq!(n.text, "abカ");
        assert_eq!(n.offsets, vec![0, 1, 4]);
    }

    #[test]
    fn test_offsets_nfkc_expansion_shares_origin() {
        // ㈱(3bytes) → "(株)" の 3 文字はすべて原文の同じ位置に由来する
        let n = normalize_with_offsets("㈱x");
        assert_eq!(n.text, "(株)x");
        assert_eq!(n.offsets, vec![0, 0, 0, 3]);
    }

    #[test]
    fn test_offsets_composed_voiced_mark() {
        // ﾃﾞ(6bytes: ﾃ=3,ﾞ=3) → デ 1 文字。由来は ﾃ の位置
        let n = normalize_with_offsets("ﾃﾞx");
        assert_eq!(n.text, "デx");
        assert_eq!(n.offsets, vec![0, 6]);
    }
}
