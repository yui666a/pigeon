import { create } from "zustand";
import type { Thread } from "../types/mail";

interface SelectionState {
  /** 選択中のスレッドID集合。選択粒度はスレッド単位（部分選択なし） */
  selectedThreadIds: Set<string>;
  toggleThread: (thread: Thread) => void;
  clear: () => void;
  isSelected: (threadId: string) => boolean;
  /** 選択中スレッドの全メールIDをフラット化して返す（一括操作の入力用）。
   * Thread 実体は持たず、呼び出し側が保持する最新の Thread[] と突き合わせる */
  selectedMailIds: (threads: Thread[]) => string[];
}

/**
 * 一覧（未分類一覧・案件別一覧・INBOX一覧）横断の複数選択状態。
 * mailStore とは独立させ、ビュー切替時は clear() で選択解除する
 * （設計書 2026-07-13-bulk-actions-design.md）
 */
export const useSelectionStore = create<SelectionState>((set, get) => ({
  selectedThreadIds: new Set(),

  toggleThread: (thread) => {
    const next = new Set(get().selectedThreadIds);
    if (next.has(thread.thread_id)) {
      next.delete(thread.thread_id);
    } else {
      next.add(thread.thread_id);
    }
    set({ selectedThreadIds: next });
  },

  clear: () => set({ selectedThreadIds: new Set() }),

  isSelected: (threadId) => get().selectedThreadIds.has(threadId),

  selectedMailIds: (threads) => {
    const selected = get().selectedThreadIds;
    return threads
      .filter((t) => selected.has(t.thread_id))
      .flatMap((t) => t.mails.map((m) => m.id));
  },
}));
