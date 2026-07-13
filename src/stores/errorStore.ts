import { create } from "zustand";

export type ToastKind = "error" | "success";

export interface Toast {
  id: string;
  kind: ToastKind;
  message: string;
  timestamp: number;
}

interface ToastState {
  toasts: Toast[];
  addError: (message: string) => void;
  addSuccess: (message: string) => void;
  dismissToast: (id: string) => void;
}

/** エラー・成功共通の自動消滅時間 */
const AUTO_DISMISS_MS = 5000;

/**
 * アプリ全体の通知トースト（エラー・操作成功）を管理するストア。
 * ToastContainer が描画し、各ストアが addError / addSuccess で発火する。
 */
export const useErrorStore = create<ToastState>((set, get) => {
  // トーストID → 自動消滅タイマー。手動 dismiss 時に clearTimeout してリークを防ぐ
  const timers = new Map<string, ReturnType<typeof setTimeout>>();

  const removeToast = (id: string) => {
    const timer = timers.get(id);
    if (timer !== undefined) {
      clearTimeout(timer);
      timers.delete(id);
    }
    set({ toasts: get().toasts.filter((t) => t.id !== id) });
  };

  const addToast = (kind: ToastKind, message: string) => {
    const id = crypto.randomUUID();
    set({
      toasts: [...get().toasts, { id, kind, message, timestamp: Date.now() }],
    });
    timers.set(
      id,
      setTimeout(() => removeToast(id), AUTO_DISMISS_MS),
    );
  };

  return {
    toasts: [],
    addError: (message) => addToast("error", message),
    addSuccess: (message) => addToast("success", message),
    dismissToast: removeToast,
  };
});
