import type { SearchMode } from "./search";

/**
 * 保存検索（スマートビュー）。Rust 側 models::saved_search::SavedSearch と
 * フィールド名を一致させるため snake_case を用いる。
 */
export interface SavedSearch {
  id: number;
  name: string;
  query: string;
  mode: SearchMode;
  sort_order: number;
  created_at: string;
}
