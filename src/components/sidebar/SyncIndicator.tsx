import { useEffect } from "react";
import { useMailStore } from "../../stores/mailStore";

export function SyncIndicator() {
  const syncProgress = useMailStore((s) => s.syncProgress);
  const initSyncListener = useMailStore((s) => s.initSyncListener);

  useEffect(() => {
    const promise = initSyncListener();
    return () => {
      promise.then((unlisten) => unlisten());
    };
  }, [initSyncListener]);

  if (!syncProgress) return null;

  return (
    <div className="border-t px-4 py-1.5 text-xs text-gray-500">
      メール同期中… {syncProgress.done.toLocaleString()} /{" "}
      {syncProgress.total.toLocaleString()}
    </div>
  );
}
