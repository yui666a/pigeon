import { useCallback, useState } from "react";

/**
 * 一覧の「もっと見る」。
 *
 * かつてはデータを全件持った上で描画だけを先頭 N 件に切る hook だったが、
 * それは転送コストを一切減らさなかった（10,000 件の本文が IPC を通っていた）。
 * ページングはサーバ側（SQL の LIMIT/OFFSET）へ移し、この hook は
 * 「続きを取りに行く」トリガと取得中状態だけを持つ（ADR 0006 決定5）。
 *
 * @param hasMore サーバ側にまだ後続があるか（ストアが持つ has_more）
 * @param fetchMore 続きを1ページ分取得してストアへ追記する関数
 */
export function useLoadMore(hasMore: boolean, fetchMore: () => Promise<void> | void) {
  const [loading, setLoading] = useState(false);

  const loadMore = useCallback(async () => {
    // 連打や多重発火で同じページを二重に取りに行かない
    if (!hasMore || loading) return;
    setLoading(true);
    try {
      await fetchMore();
    } catch {
      // 追加取得の失敗はストア側でトースト表示される。ここで再スローすると
      // onClick の未処理 rejection になるため握る（loading は必ず解除する）
    } finally {
      setLoading(false);
    }
  }, [hasMore, loading, fetchMore]);

  return { hasMore, loading, loadMore };
}
