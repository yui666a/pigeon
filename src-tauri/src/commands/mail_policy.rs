//! サーバー反映ポリシー（削除・アーカイブ・フラグ操作をサーバーへ反映するか）。
//!
//! ドメイン知識の単一の置き場所。以前は mail_commands / bulk_commands /
//! flag_commands に同じ「Sent はローカルのみ」判定が分散していたが、
//! ここに集約した（設計書 2026-07-12-mail-delete-archive-design.md、
//! 2026-07-13-bulk-actions-design.md）。
//!
//! # v1 制限: Sent の UID は信頼できない
//!
//! Sent フォルダ同期（2026-07-12-sent-sync-uidplus-design.md）により送信後の
//! Sent 行の uid は後追いで確定するが、同期前の送信直後の行は APPEND 時の
//! 推定 uid のままで、確定済みかをローカル行から判定する手段が現状ない。
//! 破壊的操作での誤爆を避けるため、Sent へのサーバー反映は安全側で
//! 一律スキップする。

use crate::models::account::AccountProvider;

/// サーバー UID を信頼できず、サーバー反映を行わないフォルダか（v1 制限）。
/// 削除・アーカイブ・フラグ操作（\Seen / \Flagged）すべての判定の根拠。
pub(crate) fn is_local_only_folder(folder: &str) -> bool {
    folder == "Sent"
}

/// 削除のサーバー反映方式（設計書 2026-07-12-mail-delete-archive-design.md）
#[derive(Debug, PartialEq)]
pub(crate) enum DeletePlan {
    /// サーバーで削除後にローカル行を削除する。
    /// サーバー側は \Trash フォルダがあればゴミ箱へ移動、なければ完全削除
    /// （imap_client::delete_message_remote 参照）
    Server,
    /// ローカル行の削除のみ（Sent。モジュールドキュメントの v1 制限参照）
    LocalOnly,
}

pub(crate) fn plan_delete(folder: &str) -> DeletePlan {
    if is_local_only_folder(folder) {
        DeletePlan::LocalOnly
    } else {
        DeletePlan::Server
    }
}

/// アーカイブのサーバー反映方式
#[derive(Debug, PartialEq)]
pub(crate) enum ArchivePlan {
    /// COPY せず \Deleted + EXPUNGE のみ（Gmail: INBOX ラベル剥がし = アーカイブ）
    DeleteOnly,
    /// archive_folder へ UID COPY してから \Deleted + EXPUNGE（一般 IMAP）
    CopyThenDelete(String),
    /// ローカルの folder 更新のみ（Sent。モジュールドキュメントの v1 制限参照）
    LocalOnly,
}

pub(crate) fn plan_archive(
    provider: &AccountProvider,
    folder: &str,
    archive_folder: &str,
) -> ArchivePlan {
    if is_local_only_folder(folder) {
        return ArchivePlan::LocalOnly;
    }
    match provider {
        AccountProvider::Google => ArchivePlan::DeleteOnly,
        AccountProvider::Other => ArchivePlan::CopyThenDelete(archive_folder.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_local_only_folder_sent() {
        assert!(is_local_only_folder("Sent"));
    }

    #[test]
    fn test_is_local_only_folder_inbox_and_archive() {
        assert!(!is_local_only_folder("INBOX"));
        assert!(!is_local_only_folder("Archive"));
    }

    #[test]
    fn test_plan_delete_inbox_requires_server() {
        assert_eq!(plan_delete("INBOX"), DeletePlan::Server);
        assert_eq!(plan_delete("Archive"), DeletePlan::Server);
    }

    #[test]
    fn test_plan_delete_sent_is_local_only() {
        // Sent の uid は APPEND 時の推定値でサーバー UID と不一致の可能性がある
        // ため v1 ではサーバー反映しない（設計書「v1 の制限」）
        assert_eq!(plan_delete("Sent"), DeletePlan::LocalOnly);
    }

    #[test]
    fn test_plan_archive_google_deletes_without_copy() {
        // Gmail は INBOX からの削除 = ラベル剥がしがアーカイブ相当
        assert_eq!(
            plan_archive(&AccountProvider::Google, "INBOX", "Archive"),
            ArchivePlan::DeleteOnly
        );
    }

    #[test]
    fn test_plan_archive_other_copies_to_archive_folder() {
        assert_eq!(
            plan_archive(&AccountProvider::Other, "INBOX", "MyArchive"),
            ArchivePlan::CopyThenDelete("MyArchive".to_string())
        );
    }

    #[test]
    fn test_plan_archive_sent_is_local_only() {
        assert_eq!(
            plan_archive(&AccountProvider::Google, "Sent", "Archive"),
            ArchivePlan::LocalOnly
        );
        assert_eq!(
            plan_archive(&AccountProvider::Other, "Sent", "Archive"),
            ArchivePlan::LocalOnly
        );
    }
}
