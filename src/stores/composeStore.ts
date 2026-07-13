import { create } from "zustand";
import type { Draft, Mail, SendMailRequest } from "../types/mail";
import type { ComposeAttachment, ComposeMode } from "../types/compose";
import { mailApi } from "../api/mailApi";
import { errorMessage } from "../api/errors";
import { buildPrefill, splitRecipients } from "../utils/composePrefill";
import type { ComposeFormat } from "../utils/composeFormat";
import { getDefaultComposeFormat } from "../utils/composeFormat";
import { htmlToPlain } from "../utils/htmlToPlain";
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
  /** 本文形式。rich のとき body は HTML、plain のときプレーンテキスト */
  format: ComposeFormat;
  /** 添付ファイル（送信時にパスを Rust へ渡す） */
  attachments: ComposeAttachment[];
  /** reply / replyAll の返信元メールID（スレッディングヘッダー導出用） */
  replyToMailId: string | null;
  /** 対応する下書きのID。自動保存または下書きから復元した場合にセットされる */
  draftId: string | null;
  openCompose: (mode: ComposeMode, sourceMail?: Mail | null) => void;
  openComposeFromDraft: (draft: Draft) => void;
  closeCompose: () => Promise<void>;
  setField: (field: ComposeField, value: string) => void;
  /** 本文形式を切り替える（rich⇔plain で body の表現を相互変換する） */
  setFormat: (format: ComposeFormat) => void;
  addAttachments: (files: ComposeAttachment[]) => void;
  removeAttachment: (path: string) => void;
  send: () => Promise<void>;
}

const EMPTY_FIELDS = {
  to: "",
  cc: "",
  bcc: "",
  subject: "",
  body: "",
  attachments: [] as ComposeAttachment[],
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
  format: getDefaultComposeFormat(),
  sending: false,

  openCompose: (mode, sourceMail = null) => {
    const account = selectedAccount();
    const prefill = buildPrefill(mode, sourceMail, account?.email ?? null);
    const isReply = mode === "reply" || mode === "replyAll";
    set({
      isOpen: true,
      mode,
      ...EMPTY_FIELDS,
      ...prefill,
      // 引用プリフィルはプレーンテキスト。既定がリッチでも、返信/転送は
      // 引用をプレーンで持ったまま開き、ユーザーが必要ならリッチへ切り替える
      format: isReply || mode === "forward" ? "plain" : getDefaultComposeFormat(),
      sending: false,
      replyToMailId: isReply && sourceMail ? sourceMail.id : null,
      draftId: null,
    });
  },

  openComposeFromDraft: (draft) => {
    set({
      isOpen: true,
      mode: "new",
      ...EMPTY_FIELDS,
      to: draft.to_addr,
      cc: draft.cc_addr,
      bcc: draft.bcc_addr,
      subject: draft.subject,
      body: draft.body_text,
      // 下書きはプレーンで保存されるため、復元は常にプレーンで開く
      format: "plain",
      sending: false,
      replyToMailId: draft.in_reply_to,
      draftId: draft.id,
    });
  },

  closeCompose: async () => {
    const account = selectedAccount();
    const { to, cc, bcc, subject, body, format, replyToMailId, draftId } = get();
    const hasInput = [to, cc, bcc, subject, body].some((v) => v.trim().length > 0);

    if (account && hasInput) {
      // v1: 下書きはプレーンで保存する（drafts テーブルは body_text のみ）。
      // リッチ本文は plain に落として保存する
      const bodyForDraft = format === "rich" ? htmlToPlain(body) : body;
      const saved = await useDraftStore.getState().saveDraft({
        id: draftId,
        account_id: account.id,
        to_addr: to,
        cc_addr: cc,
        bcc_addr: bcc,
        subject,
        body_text: bodyForDraft,
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

  setFormat: (format) => {
    const { format: current, body } = get();
    if (format === current) return;
    // rich → plain: HTML をプレーンに変換して textarea に載せる。
    // plain → rich: プレーンを段落 HTML にエスケープして載せる（改行を維持）
    const nextBody =
      format === "plain" ? htmlToPlain(body) : plainToHtml(body);
    set({ format, body: nextBody });
  },

  addAttachments: (files) => {
    const existing = get().attachments;
    const seen = new Set(existing.map((a) => a.path));
    const merged = [...existing, ...files.filter((f) => !seen.has(f.path))];
    set({ attachments: merged });
  },

  removeAttachment: (path) => {
    set({ attachments: get().attachments.filter((a) => a.path !== path) });
  },

  send: async () => {
    const account = selectedAccount();
    if (!account || get().sending) return;

    const { to, cc, bcc, subject, body, format, attachments, replyToMailId, draftId } =
      get();
    const isRich = format === "rich";
    const req: SendMailRequest = {
      account_id: account.id,
      to: splitRecipients(to),
      cc: splitRecipients(cc),
      bcc: splitRecipients(bcc),
      subject,
      // リッチ時 plain は Rust が HTML から生成するため body_text は空でよい
      body_text: isRich ? "" : body,
      reply_to_mail_id: replyToMailId,
      body_html: isRich ? body : null,
      attachments: attachments.map((a) => a.path),
    };

    set({ sending: true });
    try {
      await mailApi.sendMail(req);
      if (draftId) {
        await useDraftStore.getState().deleteDraft(draftId);
      }
      set({ isOpen: false, sending: false, ...EMPTY_FIELDS });
      useErrorStore.getState().addSuccess("メールを送信しました");
    } catch (e) {
      // 失敗時はモーダルを開いたまま入力内容を保持する
      set({ sending: false });
      useErrorStore.getState().addError(errorMessage(e));
    }
  },
}));

/** プレーンテキストを段落 HTML に変換する（plain→rich 切替時。HTML特殊文字をエスケープ） */
function plainToHtml(text: string): string {
  if (text.trim() === "") return "";
  const escape = (s: string) =>
    s
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  return text
    .split(/\n/)
    .map((line) => `<p>${line.trim() === "" ? "<br>" : escape(line)}</p>`)
    .join("");
}
