import { useState } from "react";
import { ProjectSelect } from "../common/ProjectSelect";
import type { Project } from "../../types/project";

interface ClassifyApprovalBarProps {
  projects: Project[];
  /** 現在の割り当て先。「正しい」はこの案件で確定する。
   * 空文字のときは確定先が一意に決まらないため「正しい」を出さず、
   * 移動先を選ばせる（INBOX 一覧など案件を選んでいない場面） */
  currentProjectId: string;
  onApprove: (projectId: string) => void;
  onDismiss: () => void;
}

/**
 * 確信度が中程度の AI 分類をユーザーに確認させるバー。
 * 「正しい」は現在の案件で確定し、「修正する」は移動先を選ばせる。
 *
 * どちらも同じ approve_classification に渡る。案件が変われば Rust 側が
 * 訂正として correction_log に記録し、次回以降の分類の学習材料になる
 * （設計: docs/design/2026-04-12-pigeon-design.md「⚠マーク」の項）
 */
export function ClassifyApprovalBar({
  projects,
  currentProjectId,
  onApprove,
  onDismiss,
}: ClassifyApprovalBarProps) {
  // 確定先が分からない場面では最初から選択させる
  const [correcting, setCorrecting] = useState(currentProjectId === "");

  return (
    <div className="flex flex-wrap items-center gap-2 border-b bg-yellow-50 px-6 py-2">
      <span className="text-sm text-yellow-800">
        この分類は確信度が低めです。正しいか確認してください。
      </span>
      <div className="flex flex-1 flex-wrap items-center justify-end gap-2">
        {correcting ? (
          <ProjectSelect
            projects={projects}
            ariaLabel="移動先の案件"
            placeholder="移動先を選択..."
            onSelect={onApprove}
            className="min-w-0 flex-1 rounded border px-2 py-1 text-sm"
          />
        ) : (
          <>
            <button
              type="button"
              onClick={() => onApprove(currentProjectId)}
              className="shrink-0 rounded border border-yellow-400 bg-white px-3 py-1 text-sm text-yellow-800 hover:bg-yellow-100"
            >
              正しい
            </button>
            <button
              type="button"
              onClick={() => setCorrecting(true)}
              className="shrink-0 rounded border px-3 py-1 text-sm hover:bg-yellow-100"
            >
              修正する
            </button>
          </>
        )}
        <button
          type="button"
          onClick={onDismiss}
          aria-label="閉じる"
          className="shrink-0 rounded border px-2 py-1 text-sm text-gray-500 hover:bg-gray-100"
        >
          ✕
        </button>
      </div>
    </div>
  );
}
