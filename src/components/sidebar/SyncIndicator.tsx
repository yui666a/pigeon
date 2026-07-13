import { useEffect } from "react";
import { useMailStore } from "../../stores/mailStore";

export function SyncIndicator() {
  const syncProgress = useMailStore((s) => s.syncProgress);
  const backfillProgress = useMailStore((s) => s.backfillProgress);
  const initSyncListener = useMailStore((s) => s.initSyncListener);
  const initBackfillListener = useMailStore((s) => s.initBackfillListener);

  useEffect(() => {
    const promise = initSyncListener();
    return () => {
      promise.then((unlisten) => unlisten());
    };
  }, [initSyncListener]);

  useEffect(() => {
    const promise = initBackfillListener();
    return () => {
      promise.then((unlisten) => unlisten());
    };
  }, [initBackfillListener]);

  if (!syncProgress && !backfillProgress) return null;

  return (
    <>
      {syncProgress && (
        <div className="border-t px-4 py-1.5 text-xs text-gray-500">
          メール同期中… {syncProgress.done.toLocaleString()} /{" "}
          {syncProgress.total.toLocaleString()}
        </div>
      )}
      {backfillProgress && (
        <div className="border-t px-4 py-1.5 text-xs text-gray-500">
          過去メール取得中… {backfillProgress.done.toLocaleString()} /{" "}
          {backfillProgress.total.toLocaleString()}
        </div>
      )}
    </>
  );
}
