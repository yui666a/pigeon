import { useClassifyStore } from "../../stores/classifyStore";

interface ClassifyButtonProps {
  accountId: string;
}

export function ClassifyButton({ accountId }: ClassifyButtonProps) {
  const classifying = useClassifyStore((s) => s.classifying);
  const progress = useClassifyStore((s) => s.progress);
  const classifyAll = useClassifyStore((s) => s.classifyAll);
  const cancelClassification = useClassifyStore(
    (s) => s.cancelClassification,
  );

  if (classifying) {
    return (
      <div className="flex items-center gap-2 px-4 py-2">
        <div className="flex-1">
          <div className="h-2 overflow-hidden rounded-full bg-gray-200">
            <div
              className="h-full rounded-full bg-blue-500 transition-all"
              style={{
                width: progress
                  ? `${(progress.current / progress.total) * 100}%`
                  : "0%",
              }}
            />
          </div>
          {progress && (
            <span className="mt-0.5 block text-xs text-gray-500">
              {progress.current} / {progress.total}
            </span>
          )}
        </div>
        <button
          onClick={() => cancelClassification()}
          className="shrink-0 rounded bg-gray-200 px-2 py-1 text-xs text-gray-600 hover:bg-gray-300"
        >
          キャンセル
        </button>
      </div>
    );
  }

  return (
    <div className="px-4 py-2">
      <button
        onClick={() => classifyAll(accountId)}
        className="w-full rounded bg-blue-500 px-3 py-1.5 text-sm text-white hover:bg-blue-600"
      >
        分類する
      </button>
    </div>
  );
}
