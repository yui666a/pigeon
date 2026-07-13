//! スレッド判定のドメインロジック（DB 非依存）。
//!
//! In-Reply-To / References によるグラフ結合（Union-Find）と件名フォールバックで
//! メール集合をスレッドへ分割する純粋なアルゴリズムを提供する。
//! 永続化（mails テーブルの読み書き）は `db::mails` が担い、本モジュールは
//! rusqlite に依存しない。設計:
//! docs/superpowers/specs/2026-07-13-thread-follow-classify-design.md
//! 「判定ロジックの重複実装はしない」。

use crate::models::mail::{Mail, Thread};
use std::collections::HashMap;

/// スレッド判定に必要な最小情報。
///
/// `Mail`（本文込みの完全な行）と `ThreadMailMeta`（本文を読まない軽量メタ）の
/// 両方がこれを実装し、判定コア `compute_thread_groups` を共有する。
/// これにより「軽量メタをプレースホルダ埋めの Mail に変換して渡す」ような
/// 変換を挟まずに、単一のアルゴリズムで両者を扱える。
pub trait ThreadSource {
    fn message_id(&self) -> &str;
    fn in_reply_to(&self) -> Option<&str>;
    fn references(&self) -> Option<&str>;
    fn subject(&self) -> &str;
    fn date(&self) -> &str;
}

impl ThreadSource for Mail {
    fn message_id(&self) -> &str {
        &self.message_id
    }
    fn in_reply_to(&self) -> Option<&str> {
        self.in_reply_to.as_deref()
    }
    fn references(&self) -> Option<&str> {
        self.references.as_deref()
    }
    fn subject(&self) -> &str {
        &self.subject
    }
    fn date(&self) -> &str {
        &self.date
    }
}

/// スレッド判定に必要な最小カラムのみの軽量メールデータ。
/// `auto_follow_threads` のように本文を使わない処理が、body_text/body_html を
/// 読み込まずに済ませるための構造体。
#[derive(Debug, Clone)]
pub struct ThreadMailMeta {
    pub id: String,
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub subject: String,
    pub date: String,
}

impl ThreadSource for ThreadMailMeta {
    fn message_id(&self) -> &str {
        &self.message_id
    }
    fn in_reply_to(&self) -> Option<&str> {
        self.in_reply_to.as_deref()
    }
    fn references(&self) -> Option<&str> {
        self.references.as_deref()
    }
    fn subject(&self) -> &str {
        &self.subject
    }
    fn date(&self) -> &str {
        &self.date
    }
}

/// 判定コア: メール集合をスレッドへ分割し、添字のグループとして返す。
///
/// - In-Reply-To / References で参照し合うメールを Union-Find で結合する
/// - ヘッダを持たないメールは正規化件名の一致でフォールバック結合する
/// - 各グループ内の添字は date 昇順、グループ同士は最終メールの date 降順
fn compute_thread_groups<T: ThreadSource>(items: &[T]) -> Vec<Vec<usize>> {
    let mut by_message_id: HashMap<&str, usize> = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        by_message_id.insert(item.message_id(), i);
    }

    let mut thread_root: Vec<usize> = (0..items.len()).collect();

    for (i, item) in items.iter().enumerate() {
        if let Some(reply_to) = item.in_reply_to() {
            if let Some(&parent_idx) = by_message_id.get(reply_to) {
                union(&mut thread_root, i, parent_idx);
            }
        }
        if let Some(refs) = item.references() {
            for ref_id in refs.split_whitespace() {
                if let Some(&ref_idx) = by_message_id.get(ref_id) {
                    union(&mut thread_root, i, ref_idx);
                }
            }
        }
    }

    // 件名フォールバック: ヘッダ（In-Reply-To/References）を持たないメールを、
    // 同じ正規化件名が最初に現れたメールと結合する。照合先はヘッダの有無を問わない。
    // 「件名 → 初出添字」のマップで先行メールを O(1) 参照する（旧実装は
    // 先行メールの線形走査で O(n²) だった）
    let normalized: Vec<String> = items
        .iter()
        .map(|m| normalize_subject(m.subject()))
        .collect();
    let mut first_by_subject: HashMap<&str, usize> = HashMap::new();
    for i in 0..items.len() {
        if items[i].in_reply_to().is_none() && items[i].references().is_none() {
            if let Some(&j) = first_by_subject.get(normalized[i].as_str()) {
                union(&mut thread_root, i, j);
            }
        }
        first_by_subject.entry(&normalized[i]).or_insert(i);
    }

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..items.len() {
        let root = find_root(&thread_root, i);
        groups.entry(root).or_default().push(i);
    }

    let mut result: Vec<Vec<usize>> = groups.into_values().collect();
    for indices in &mut result {
        indices.sort_by(|&a, &b| items[a].date().cmp(items[b].date()));
    }
    // グループ同士は最終メール（グループ末尾）の date 降順
    result.sort_by(|a, b| {
        let last_a = a.last().map(|&i| items[i].date()).unwrap_or_default();
        let last_b = b.last().map(|&i| items[i].date()).unwrap_or_default();
        last_b.cmp(last_a)
    });
    result
}

/// i の根と j の根を結合する（i 側の根を j 側の根へ付け替える）。
fn union(thread_root: &mut [usize], i: usize, j: usize) {
    let root_i = find_root(thread_root, i);
    let root_j = find_root(thread_root, j);
    if root_i != root_j {
        thread_root[root_i] = root_j;
    }
}

fn find_root(roots: &[usize], mut i: usize) -> usize {
    while roots[i] != i {
        i = roots[i];
    }
    i
}

/// メール集合をスレッドへ分割する。判定は `compute_thread_groups` に委譲し、
/// ここでは UI 表示用の `Thread`（件名・最終日時・参加者一覧）へ組み立てる。
///
/// 所有権を取り、メールをクローンせずにスレッドへ移動する。借用スライスしか
/// 持てない呼び出し側には `db::mails::build_threads`（互換ラッパー）がある。
pub fn build_threads(mails: Vec<Mail>) -> Vec<Thread> {
    let groups = compute_thread_groups(&mails);
    // 添字グループに従って Vec から所有権ごと取り出す（グループは互いに素なので
    // 各スロットはちょうど1回だけ take される）
    let mut slots: Vec<Option<Mail>> = mails.into_iter().map(Some).collect();
    groups
        .into_iter()
        .map(|indices| {
            let thread_mails: Vec<Mail> = indices.iter().filter_map(|&i| slots[i].take()).collect();
            let from_addrs: Vec<String> = thread_mails
                .iter()
                .map(|m| m.from_addr.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            let last_date = thread_mails
                .last()
                .map(|m| m.date.clone())
                .unwrap_or_default();
            let subject = thread_mails
                .first()
                .map(|m| m.subject.clone())
                .unwrap_or_default();
            let thread_id = thread_mails
                .first()
                .map(|m| m.message_id.clone())
                .unwrap_or_default();
            Thread {
                thread_id,
                subject,
                last_date,
                mail_count: thread_mails.len(),
                from_addrs,
                mails: thread_mails,
            }
        })
        .collect()
}

/// 軽量メタをスレッドに分割し、スレッドごとのメールID集合を返す。
/// 判定は `compute_thread_groups`（`build_threads` と同一のコア）に委譲する。
pub fn group_mail_ids_into_threads(metas: &[ThreadMailMeta]) -> Vec<Vec<String>> {
    compute_thread_groups(metas)
        .into_iter()
        .map(|indices| indices.into_iter().map(|i| metas[i].id.clone()).collect())
        .collect()
}

fn normalize_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let lower = s.to_lowercase();
        if lower.starts_with("re:") || lower.starts_with("fw:") {
            s = s[3..].trim_start();
        } else if lower.starts_with("fwd:") {
            s = s[4..].trim_start();
        } else {
            break;
        }
    }
    s.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::make_mail;

    #[test]
    fn test_build_threads_by_in_reply_to() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Hello", "2026-04-13T10:00:00");
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Re: Hello", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<msg1@ex.com>".into());
        let threads = build_threads(vec![m1, m2]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 2);
    }

    #[test]
    fn test_build_threads_by_references() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Topic", "2026-04-13T10:00:00");
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Re: Topic", "2026-04-13T11:00:00");
        m2.references = Some("<msg1@ex.com>".into());
        let mut m3 = make_mail(
            "m3",
            "<msg3@ex.com>",
            "Re: Re: Topic",
            "2026-04-13T12:00:00",
        );
        m3.references = Some("<msg1@ex.com> <msg2@ex.com>".into());
        let threads = build_threads(vec![m1, m2, m3]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 3);
    }

    #[test]
    fn test_build_threads_subject_fallback() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "見積もりの件", "2026-04-13T10:00:00");
        let m2 = make_mail(
            "m2",
            "<msg2@ex.com>",
            "Re: 見積もりの件",
            "2026-04-13T11:00:00",
        );
        let threads = build_threads(vec![m1, m2]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 2);
    }

    #[test]
    fn test_build_threads_separate() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Topic A", "2026-04-13T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "Topic B", "2026-04-13T11:00:00");
        let threads = build_threads(vec![m1, m2]);
        assert_eq!(threads.len(), 2);
    }

    #[test]
    fn test_normalize_subject() {
        assert_eq!(normalize_subject("Re: Hello"), "hello");
        assert_eq!(normalize_subject("Fwd: Re: Hello"), "hello");
        assert_eq!(normalize_subject("FW: Hello"), "hello");
        assert_eq!(normalize_subject("Hello"), "hello");
    }

    #[test]
    fn test_build_threads_empty() {
        let threads = build_threads(vec![]);
        assert!(threads.is_empty());
    }

    #[test]
    fn test_build_threads_single_mail() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Solo", "2026-04-13T10:00:00");
        let threads = build_threads(vec![m1]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 1);
        assert_eq!(threads[0].subject, "Solo");
    }

    #[test]
    fn test_build_threads_sorted_by_last_date_desc() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Old Topic", "2026-04-10T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "New Topic", "2026-04-13T10:00:00");
        let threads = build_threads(vec![m1, m2]);
        assert_eq!(threads.len(), 2);
        assert_eq!(threads[0].subject, "New Topic");
        assert_eq!(threads[1].subject, "Old Topic");
    }

    #[test]
    fn test_build_threads_fw_prefix_groups() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "案件の件", "2026-04-13T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "Fw: 案件の件", "2026-04-13T11:00:00");
        let threads = build_threads(vec![m1, m2]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 2);
    }

    #[test]
    fn test_build_threads_fwd_prefix_groups() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Report", "2026-04-13T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "Fwd: Report", "2026-04-13T11:00:00");
        let threads = build_threads(vec![m1, m2]);
        assert_eq!(threads.len(), 1);
    }

    #[test]
    fn test_build_threads_deep_chain() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Topic", "2026-04-13T10:00:00");
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Re: Topic", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<msg1@ex.com>".into());
        let mut m3 = make_mail(
            "m3",
            "<msg3@ex.com>",
            "Re: Re: Topic",
            "2026-04-13T12:00:00",
        );
        m3.in_reply_to = Some("<msg2@ex.com>".into());
        let mut m4 = make_mail(
            "m4",
            "<msg4@ex.com>",
            "Re: Re: Re: Topic",
            "2026-04-13T13:00:00",
        );
        m4.in_reply_to = Some("<msg3@ex.com>".into());
        let threads = build_threads(vec![m1, m2, m3, m4]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 4);
    }

    #[test]
    fn test_build_threads_from_addrs_deduplication() {
        let mut m1 = make_mail("m1", "<msg1@ex.com>", "Topic", "2026-04-13T10:00:00");
        m1.from_addr = "alice@example.com".into();
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Re: Topic", "2026-04-13T11:00:00");
        m2.from_addr = "alice@example.com".into();
        m2.in_reply_to = Some("<msg1@ex.com>".into());
        let threads = build_threads(vec![m1, m2]);
        assert_eq!(threads[0].from_addrs.len(), 1);
    }

    #[test]
    fn test_build_threads_subject_grouping_skipped_when_has_references() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Same Subject", "2026-04-13T10:00:00");
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Same Subject", "2026-04-13T11:00:00");
        m2.references = Some("<nonexistent@ex.com>".into());
        let threads = build_threads(vec![m1, m2]);
        assert_eq!(threads.len(), 2);
    }

    #[test]
    fn test_build_threads_subject_fallback_joins_first_match() {
        // 件名フォールバックは「最初に同じ正規化件名が現れたメール」と結合する。
        // ヘッダなしの同名メールが3通あっても推移的に1スレッドへまとまる
        let m1 = make_mail("m1", "<msg1@ex.com>", "Topic", "2026-04-13T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "Re: Topic", "2026-04-13T11:00:00");
        let m3 = make_mail("m3", "<msg3@ex.com>", "Fwd: Topic", "2026-04-13T12:00:00");
        let threads = build_threads(vec![m1, m2, m3]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 3);
    }

    #[test]
    fn test_build_threads_headered_mail_seeds_subject_fallback() {
        // References を持つメールが先行する場合でも、後続のヘッダなしメールは
        // 件名フォールバックでそのメールと結合できる（フォールバックの照合先は
        // ヘッダの有無を問わない）
        let mut m1 = make_mail("m1", "<msg1@ex.com>", "Topic", "2026-04-13T10:00:00");
        m1.references = Some("<nonexistent@ex.com>".into());
        let m2 = make_mail("m2", "<msg2@ex.com>", "Re: Topic", "2026-04-13T11:00:00");
        let threads = build_threads(vec![m1, m2]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 2);
    }

    #[test]
    fn test_normalize_subject_nested_prefixes() {
        assert_eq!(normalize_subject("Re: Fw: Re: Hello"), "hello");
        assert_eq!(normalize_subject("FW: FWD: RE: Hello"), "hello");
    }

    #[test]
    fn test_normalize_subject_case_insensitive() {
        assert_eq!(normalize_subject("RE: HELLO"), "hello");
        assert_eq!(normalize_subject("re: hello"), "hello");
    }

    #[test]
    fn test_normalize_subject_whitespace() {
        assert_eq!(normalize_subject("  Re:   Hello  "), "hello");
    }

    // --- スレッド判定用の軽量メタ（auto_follow_threads の本文ロード回避用） ---

    /// テスト補助: Mail からスレッド判定用の軽量メタを作る
    fn meta_of(m: &Mail) -> ThreadMailMeta {
        ThreadMailMeta {
            id: m.id.clone(),
            message_id: m.message_id.clone(),
            in_reply_to: m.in_reply_to.clone(),
            references: m.references.clone(),
            subject: m.subject.clone(),
            date: m.date.clone(),
        }
    }

    #[test]
    fn test_group_mail_ids_into_threads_links_replies_and_subject_fallback() {
        // In-Reply-To 結合・References 結合・件名フォールバックが
        // build_threads と同じ意味論で効くこと
        let m1 = make_mail("m1", "<msg1@ex.com>", "Topic A", "2026-04-13T10:00:00");
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Re: Topic A", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<msg1@ex.com>".into());
        let mut m3 = make_mail("m3", "<msg3@ex.com>", "Re: Topic A", "2026-04-13T12:00:00");
        m3.references = Some("<msg1@ex.com> <msg2@ex.com>".into());
        // ヘッダなし・正規化件名一致 → 件名フォールバックで m1 と同じスレッド
        let m4 = make_mail("m4", "<msg4@ex.com>", "Fwd: topic a", "2026-04-13T13:00:00");
        // 無関係なメールは独立したスレッド
        let m5 = make_mail("m5", "<msg5@ex.com>", "Unrelated", "2026-04-13T14:00:00");

        // date DESC（専用クエリと同じ順序）で渡す
        let metas: Vec<ThreadMailMeta> = [&m5, &m4, &m3, &m2, &m1]
            .iter()
            .map(|m| meta_of(m))
            .collect();
        let groups = group_mail_ids_into_threads(&metas);

        let sets: std::collections::HashSet<std::collections::BTreeSet<String>> = groups
            .into_iter()
            .map(|ids| ids.into_iter().collect())
            .collect();
        let expected: std::collections::HashSet<std::collections::BTreeSet<String>> = [
            ["m1", "m2", "m3", "m4"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            ["m5"].iter().map(|s| s.to_string()).collect(),
        ]
        .into_iter()
        .collect();
        assert_eq!(sets, expected);
    }

    #[test]
    fn test_group_mail_ids_into_threads_matches_build_threads() {
        // 回帰ガード: build_threads とスレッド分割が一致すること
        // （判定ロジックが将来分岐・重複実装されるのを検知する）
        let m1 = make_mail("m1", "<msg1@ex.com>", "Topic A", "2026-04-13T10:00:00");
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Re: Topic A", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<msg1@ex.com>".into());
        let mut m3 = make_mail("m3", "<msg3@ex.com>", "Re: Topic A", "2026-04-13T12:00:00");
        m3.references = Some("<nonexistent@ex.com> <msg2@ex.com>".into());
        let m4 = make_mail("m4", "<msg4@ex.com>", "Topic B", "2026-04-13T13:00:00");
        let m5 = make_mail("m5", "<msg5@ex.com>", "FW: topic b", "2026-04-13T14:00:00");
        let mails = vec![m5, m4, m3, m2, m1];

        let metas: Vec<ThreadMailMeta> = mails.iter().map(meta_of).collect();
        let actual: std::collections::HashSet<std::collections::BTreeSet<String>> =
            group_mail_ids_into_threads(&metas)
                .into_iter()
                .map(|ids| ids.into_iter().collect())
                .collect();
        let expected: std::collections::HashSet<std::collections::BTreeSet<String>> =
            build_threads(mails)
                .into_iter()
                .map(|t| t.mails.into_iter().map(|m| m.id).collect())
                .collect();
        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 2);
    }
}
