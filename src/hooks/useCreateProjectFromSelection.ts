import { useState } from "react";
import { useProjectStore } from "../stores/projectStore";
import { useMailStore } from "../stores/mailStore";
import { useSelectionStore } from "../stores/selectionStore";
import type { Thread } from "../types/mail";

interface UseCreateProjectFromSelectionOptions {
  /** 作成対象のアカウント。null の間は submit しても何もしない */
  accountId: string | null;
  /** 選択スレッドIDとの突き合わせに使う、表示中の最新スレッド一覧 */
  threads: Thread[];
  /** 作成＋移動の成功後に呼ぶ一覧再読み込み（呼び出し元のビューに依存） */
  reload: () => void;
}

/**
 * 選択メールから新規案件を作成し、そのメールを案件へ一括移動するフロー。
 * UnclassifiedList / ThreadList 共通。フォームを開いた時点の選択メールIDを
 * 固定し（提案と作成の対象を一致させる）、submit で
 * createProject → bulkMoveMails を順に実行する
 * （設計書 2026-07-17-group-unclassified-into-new-project-design.md）。
 */
export function useCreateProjectFromSelection({
  accountId,
  threads,
  reload,
}: UseCreateProjectFromSelectionOptions) {
  const createProject = useProjectStore((s) => s.createProject);
  const bulkMoveMails = useMailStore((s) => s.bulkMoveMails);
  const selectedMailIds = useSelectionStore((s) => s.selectedMailIds);
  const clearSelection = useSelectionStore((s) => s.clear);

  const [creating, setCreating] = useState(false);
  const [formMailIds, setFormMailIds] = useState<string[]>([]);

  // 「＋ 新しい案件」押下: 現在の選択メールを固定してフォームを開く
  const open = () => {
    const mailIds = selectedMailIds(threads);
    if (mailIds.length === 0) return;
    setFormMailIds(mailIds);
    setCreating(true);
  };

  const cancel = () => {
    setCreating(false);
    setFormMailIds([]);
  };

  // フォーム確定: 案件を作成し、固定した選択メールをその案件へ移動する
  const submit = async (name: string, description: string | undefined) => {
    if (!accountId) return;
    const project = await createProject(accountId, name, description);
    await bulkMoveMails(formMailIds, project.id);
    clearSelection();
    setCreating(false);
    setFormMailIds([]);
    reload();
  };

  return { creating, formMailIds, open, cancel, submit };
}
