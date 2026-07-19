import {
  CONFIDENCE_AUTO_ASSIGN,
  CONFIDENCE_UNCERTAIN,
} from "../../utils/classifyConfidence";

interface ClassifyResultBadgeProps {
  confidence: number;
  assignedBy: string;
  /** 渡すとバッジがボタンになる。要確認の分類を承認/修正させる導線に使う。
   * 一覧など操作を伴わない場所では省略して非対話の表示にする */
  onClick?: () => void;
}

export function ClassifyResultBadge({
  confidence,
  assignedBy,
  onClick,
}: ClassifyResultBadgeProps) {
  if (assignedBy === "user") return null;

  /** onClick があるときだけ button として描画する（無ければ span のまま） */
  const wrap = (className: string, children: React.ReactNode) =>
    onClick ? (
      <button
        type="button"
        aria-label="AI分類を確認"
        onClick={onClick}
        className={`${className} hover:brightness-95`}
      >
        {children}
      </button>
    ) : (
      <span className={className}>{children}</span>
    );

  if (confidence >= CONFIDENCE_AUTO_ASSIGN) {
    return wrap(
      "inline-flex items-center rounded-full bg-green-100 px-1.5 py-0.5 text-xs font-medium text-green-700",
      "AI",
    );
  }

  if (confidence >= CONFIDENCE_UNCERTAIN) {
    return wrap(
      "inline-flex items-center gap-0.5 rounded-full bg-yellow-100 px-1.5 py-0.5 text-xs font-medium text-yellow-700",
      <>
        <svg
          className="h-3 w-3"
          fill="none"
          viewBox="0 0 24 24"
          strokeWidth={2}
          stroke="currentColor"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"
          />
        </svg>
        AI
      </>,
    );
  }

  return null;
}
