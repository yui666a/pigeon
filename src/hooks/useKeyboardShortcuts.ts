import { useEffect } from "react";
import { useComposeStore } from "../stores/composeStore";
import { useMailStore } from "../stores/mailStore";
import type { ComposeMode } from "../utils/composePrefill";
import type { Mail } from "../types/mail";

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
 * メール操作のキーボードショートカット（App直下で有効化する）。
 * n: 新規作成 / r: 返信 / a: 全員に返信 / f: 転送 / e: アーカイブ
 */
export function useKeyboardShortcuts() {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (isTextInput(e.target)) return;
      if (useComposeStore.getState().isOpen) return;

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
