import { useCallback, useEffect, useState } from "react";

const PAGE_SIZE = 200;

/**
 * 大量リストの描画ページング。データは全件持ち、描画だけを
 * 先頭 PAGE_SIZE 件 + 「もっと見る」で切る（仮想化ライブラリは使わない）。
 */
export function useDisplayLimit<T>(items: T[], resetKey: unknown) {
  const [limit, setLimit] = useState(PAGE_SIZE);

  useEffect(() => {
    setLimit(PAGE_SIZE);
  }, [resetKey]);

  const showMore = useCallback(() => setLimit((l) => l + PAGE_SIZE), []);

  return {
    visible: items.slice(0, limit),
    hasMore: items.length > limit,
    remaining: Math.max(0, items.length - limit),
    showMore,
  };
}
