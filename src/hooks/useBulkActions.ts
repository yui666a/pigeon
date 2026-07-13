import { useMailStore } from "../stores/mailStore";
import { useSelectionStore } from "../stores/selectionStore";
import type { Thread } from "../types/mail";

interface UseBulkActionsOptions {
  /** 削除・アーカイブの対象アカウント。null の間は両操作とも何もしない */
  accountId: string | null;
  /** 選択スレッドIDとの突き合わせに使う、表示中の最新スレッド一覧 */
  threads: Thread[];
  /** 操作成功後の一覧再読み込み（呼び出し元のビューに依存するため注入する） */
  reload: () => void;
}

/**
 * 一覧（INBOX・案件別・未分類）共通の一括操作ハンドラ。
 * 確認ダイアログ文言・selectionStore の選択解除・リロードの流れを一元化する
 * （設計書 2026-07-13-bulk-actions-design.md）
 */
export function useBulkActions({ accountId, threads, reload }: UseBulkActionsOptions) {
  const bulkDeleteMails = useMailStore((s) => s.bulkDeleteMails);
  const bulkArchiveMails = useMailStore((s) => s.bulkArchiveMails);
  const bulkMoveMails = useMailStore((s) => s.bulkMoveMails);
  const selectedThreadIds = useSelectionStore((s) => s.selectedThreadIds);
  const selectedMailIds = useSelectionStore((s) => s.selectedMailIds);
  const clearSelection = useSelectionStore((s) => s.clear);

  const handleBulkDelete = async () => {
    if (!accountId) return;
    const mailIds = selectedMailIds(threads);
    if (mailIds.length === 0) return;
    if (
      !window.confirm(
        `選択した ${selectedThreadIds.size} スレッドを削除しますか？サーバーにゴミ箱があればゴミ箱へ移動し、無い場合は完全に削除されます。`,
      )
    ) {
      return;
    }
    await bulkDeleteMails(accountId, mailIds);
    clearSelection();
    reload();
  };

  const handleBulkArchive = async () => {
    if (!accountId) return;
    const mailIds = selectedMailIds(threads);
    if (mailIds.length === 0) return;
    await bulkArchiveMails(accountId, mailIds);
    clearSelection();
    reload();
  };

  const handleBulkMove = async (projectId: string) => {
    const mailIds = selectedMailIds(threads);
    if (mailIds.length === 0) return;
    await bulkMoveMails(mailIds, projectId);
    clearSelection();
    reload();
  };

  return {
    handleBulkDelete,
    handleBulkArchive,
    handleBulkMove,
    /** BulkActionBar 表示用の選択スレッド数 */
    selectedCount: selectedThreadIds.size,
    clearSelection,
  };
}
