import { useEffect } from "react";
import { SEARCH_INPUT_ID } from "../components/sidebar/SearchBar";
import { useComposeStore } from "../stores/composeStore";
import { useMailStore } from "../stores/mailStore";
import type { ComposeMode } from "../utils/composePrefill";
import type { Mail } from "../types/mail";

const NAV_SHORTCUTS: Record<string, 1 | -1> = {
  j: 1,
  k: -1,
};

const MAIL_SHORTCUTS: Record<string, ComposeMode> = {
  r: "reply",
  a: "replyAll",
  f: "forward",
};

/** テキスト入力中はショートカットを無効にする */
function isTextInput(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    (target instanceof HTMLElement && target.isContentEditable)
  );
}

/** 選択中メール（なければ選択中スレッドの最新メール）を返す */
function targetMail(): Mail | null {
  const { selectedMail, selectedThread } = useMailStore.getState();
  if (selectedMail) return selectedMail;
  const mails = selectedThread?.mails;
  return mails && mails.length > 0 ? mails[mails.length - 1] : null;
}

/**
 * 選択中スレッド内のメールを前後に移動する。境界で止まる（ループしない）。
 * スレッド未選択時はスレッド一覧を移動する（未選択からの「次」= 先頭）。
 * 選択は selectMail / selectThread 経由のため、既存の既読化フローに乗る。
 */
function navigateMail(direction: 1 | -1): void {
  const { selectedThread, selectedMail, threads, selectMail, selectThread } =
    useMailStore.getState();

  if (selectedThread) {
    const mails = selectedThread.mails;
    // メール未選択時は末尾（最新）が本文表示されている（MailView と同じ規約）
    const currentIndex = selectedMail
      ? mails.findIndex((m) => m.id === selectedMail.id)
      : mails.length - 1;
    if (currentIndex === -1) return;
    const next = mails[currentIndex + direction];
    if (next) selectMail(next);
    return;
  }

  // スレッド未選択: 「次」で先頭スレッドを選択。「前」は存在しないので止まる
  if (direction === 1 && threads.length > 0) {
    selectThread(threads[0]);
  }
}

/**
 * メール操作のキーボードショートカット（App直下で有効化する）。
 * n: 新規作成 / r: 返信 / a: 全員に返信 / f: 転送 / e: アーカイブ /
 * j・k: 次・前のメール / /: 検索にフォーカス
 */
export function useKeyboardShortcuts() {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (isTextInput(e.target)) return;
      if (useComposeStore.getState().isOpen) return;

      const navDirection = NAV_SHORTCUTS[e.key];
      if (navDirection) {
        e.preventDefault();
        navigateMail(navDirection);
        return;
      }
      if (e.key === "/") {
        e.preventDefault();
        document.getElementById(SEARCH_INPUT_ID)?.focus();
        return;
      }

      const openCompose = useComposeStore.getState().openCompose;
      if (e.key === "n") {
        e.preventDefault();
        openCompose("new");
        return;
      }
      if (e.key === "e") {
        const mail = targetMail();
        if (!mail) return;
        e.preventDefault();
        void useMailStore.getState().archiveMail(mail);
        return;
      }
      const mode = MAIL_SHORTCUTS[e.key];
      if (mode) {
        const mail = targetMail();
        if (!mail) return;
        e.preventDefault();
        openCompose(mode, mail);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);
}
