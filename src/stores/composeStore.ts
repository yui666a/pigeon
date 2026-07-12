import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Mail, SendMailRequest } from "../types/mail";
import type { ComposeMode } from "../utils/composePrefill";
import { buildPrefill, splitRecipients } from "../utils/composePrefill";
import { useAccountStore } from "./accountStore";
import { useErrorStore } from "./errorStore";

type ComposeField = "to" | "cc" | "bcc" | "subject" | "body";

interface ComposeState {
  isOpen: boolean;
  mode: ComposeMode;
  /** 宛先はカンマ区切り文字列で保持し、送信時に配列へ分割する */
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  body: string;
  sending: boolean;
  /** reply / replyAll の返信元メールID（スレッディングヘッダー導出用） */
  replyToMailId: string | null;
  openCompose: (mode: ComposeMode, sourceMail?: Mail | null) => void;
  closeCompose: () => void;
  setField: (field: ComposeField, value: string) => void;
  send: () => Promise<void>;
}

const EMPTY_FIELDS = {
  to: "",
  cc: "",
  bcc: "",
  subject: "",
  body: "",
  replyToMailId: null as string | null,
};

function selectedAccount() {
  const { accounts, selectedAccountId } = useAccountStore.getState();
  return accounts.find((a) => a.id === selectedAccountId) ?? null;
}

export const useComposeStore = create<ComposeState>((set, get) => ({
  isOpen: false,
  mode: "new",
  ...EMPTY_FIELDS,
  sending: false,

  openCompose: (mode, sourceMail = null) => {
    const account = selectedAccount();
    const prefill = buildPrefill(mode, sourceMail, account?.email ?? null);
    const isReply = mode === "reply" || mode === "replyAll";
    set({
      isOpen: true,
      mode,
      ...prefill,
      sending: false,
      replyToMailId: isReply && sourceMail ? sourceMail.id : null,
    });
  },

  closeCompose: () => {
    set({ isOpen: false, sending: false, ...EMPTY_FIELDS });
  },

  setField: (field, value) => set({ [field]: value }),

  send: async () => {
    const account = selectedAccount();
    if (!account || get().sending) return;

    const { to, cc, bcc, subject, body, replyToMailId } = get();
    const req: SendMailRequest = {
      account_id: account.id,
      to: splitRecipients(to),
      cc: splitRecipients(cc),
      bcc: splitRecipients(bcc),
      subject,
      body_text: body,
      reply_to_mail_id: replyToMailId,
    };

    set({ sending: true });
    try {
      await invoke("send_mail", { req });
      set({ isOpen: false, sending: false, ...EMPTY_FIELDS });
    } catch (e) {
      // 失敗時はモーダルを開いたまま入力内容を保持する
      set({ sending: false });
      useErrorStore.getState().addError(String(e));
    }
  },
}));
