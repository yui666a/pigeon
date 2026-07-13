import { invokeCommand } from "./client";
import type { Account, CreateAccountRequest } from "../types/account";

/** アカウント・OAuth 系 Tauri commands の型付きラッパ */
export const accountApi = {
  fetchAccounts: () => invokeCommand<Account[]>("get_accounts"),

  createAccount: (request: CreateAccountRequest) =>
    invokeCommand<Account>("create_account", { request }),

  removeAccount: (id: string) => invokeCommand<void>("remove_account", { id }),

  /** 認可 URL を返す。accountId を渡すと既存アカウントの再認証になる */
  startOAuth: (provider: string, accountId?: string) =>
    invokeCommand<string>(
      "start_oauth",
      accountId === undefined ? { provider } : { provider, accountId },
    ),

  handleOAuthCallback: (url: string) =>
    invokeCommand<void>("handle_oauth_callback", { url }),
};
