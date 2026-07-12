import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Draft, Mail, SendMailRequest } from "../types/mail";
import type { ComposeMode } from "../utils/composePrefill";
import { buildPrefill, splitRecipients } from "../utils/composePrefill";
import { useAccountStore } from "./accountStore";
import { useDraftStore } from "./draftStore";
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
  /** 対応する下書きのID。自動保存または下書きから復元した場合にセットされる */
  draftId: string | null;
  openCompose: (mode: ComposeMode, sourceMail?: Mail | null) => void;
  openComposeFromDraft: (draft: Draft) => void;
  closeCompose: () => Promise<void>;
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
  draftId: null as string | null,
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
      draftId: null,
    });
  },

  openComposeFromDraft: (draft) => {
    set({
      isOpen: true,
      mode: "new",
      to: draft.to_addr,
      cc: draft.cc_addr,
      bcc: draft.bcc_addr,
      subject: draft.subject,
      body: draft.body_text,
      sending: false,
      replyToMailId: draft.in_reply_to,
      draftId: draft.id,
    });
  },

  closeCompose: async () => {
    const account = selectedAccount();
    const { to, cc, bcc, subject, body, replyToMailId, draftId } = get();
    const hasInput = [to, cc, bcc, subject, body].some((v) => v.trim().length > 0);

    if (account && hasInput) {
      const saved = await useDraftStore.getState().saveDraft({
        id: draftId,
        account_id: account.id,
        to_addr: to,
        cc_addr: cc,
        bcc_addr: bcc,
        subject,
        body_text: body,
        in_reply_to: replyToMailId,
      });
      // 保存失敗時（saved === null）は draftId を維持し、次回クローズ時に再試行させる
      if (saved) {
        set({ draftId: saved.id });
      }
    }

    set({ isOpen: false, sending: false, ...EMPTY_FIELDS });
  },

  setField: (field, value) => set({ [field]: value }),

  send: async () => {
    const account = selectedAccount();
    if (!account || get().sending) return;

    const { to, cc, bcc, subject, body, replyToMailId, draftId } = get();
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
      if (draftId) {
        await useDraftStore.getState().deleteDraft(draftId);
      }
      set({ isOpen: false, sending: false, ...EMPTY_FIELDS });
      useErrorStore.getState().addSuccess("メールを送信しました");
    } catch (e) {
      // 失敗時はモーダルを開いたまま入力内容を保持する
      set({ sending: false });
      useErrorStore.getState().addError(String(e));
    }
  },
}));
