interface ClassifyResultBadgeProps {
  confidence: number;
  assignedBy: string;
}

export function ClassifyResultBadge({
  confidence,
  assignedBy,
}: ClassifyResultBadgeProps) {
  if (assignedBy === "user") return null;

  if (confidence >= 0.7) {
    return (
      <span className="inline-flex items-center rounded-full bg-green-100 px-1.5 py-0.5 text-xs font-medium text-green-700">
        AI
      </span>
    );
  }

  if (confidence >= 0.4) {
    return (
      <span className="inline-flex items-center gap-0.5 rounded-full bg-yellow-100 px-1.5 py-0.5 text-xs font-medium text-yellow-700">
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
      </span>
    );
  }

  return null;
}
