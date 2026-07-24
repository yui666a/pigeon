import type { MailPreview } from "../../types/embeddingMap";

interface Props {
  preview: MailPreview | null;
  loading: boolean;
}

export function PreviewPane({ preview, loading }: Props) {
  if (loading) return <div className="p-4 text-sm text-gray-500">読み込み中...</div>;
  if (!preview)
    return <div className="p-4 text-sm text-gray-400">点をクリックするとメールの概要が出ます</div>;
  return (
    <div className="p-4 space-y-2 overflow-y-auto">
      <div className="font-semibold text-sm">{preview.subject}</div>
      <div className="text-xs text-gray-500">{preview.from_addr}</div>
      <div className="text-xs text-gray-400">{preview.date}</div>
      <div className="text-sm whitespace-pre-wrap border-t pt-2">{preview.body_excerpt}</div>
    </div>
  );
}
