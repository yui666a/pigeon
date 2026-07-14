import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Account, CreateAccountRequest, OAuthStatus } from "../types/account";
import { accountApi } from "../api/accountApi";
import { errorMessage } from "../api/errors";
import { isOAuthCallbackUrl } from "../utils/oauthCallback";
import { useErrorStore } from "./errorStore";

interface AccountState {
  accounts: Account[];
  selectedAccountId: string | null;
  loading: boolean;
  oauthStatus: OAuthStatus;
  oauthError: string | null;
  reauthAccountId: string | null;
  fetchAccounts: () => Promise<void>;
  createAccount: (req: CreateAccountRequest) => Promise<void>;
  removeAccount: (id: string) => Promise<void>;
  selectAccount: (id: string | null) => void;
  startOAuth: (provider: string) => Promise<void>;
  startReauth: (accountId: string) => Promise<void>;
  handleOAuthCallback: (url: string) => Promise<void>;
  resetOAuth: () => void;
  initDeepLinkListener: () => Promise<() => void>;
}

export const useAccountStore = create<AccountState>((set, get) => ({
  accounts: [],
  selectedAccountId: null,
  loading: false,
  oauthStatus: "idle",
  oauthError: null,
  reauthAccountId: null,

  fetchAccounts: async () => {
    set({ loading: true });
    try {
      const accounts = await accountApi.fetchAccounts();
      set({ accounts, loading: false });
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  createAccount: async (req) => {
    set({ loading: true });
    try {
      await accountApi.createAccount(req);
      const accounts = await accountApi.fetchAccounts();
      set({ accounts, loading: false });
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  removeAccount: async (id) => {
    set({ loading: true });
    try {
      await accountApi.removeAccount(id);
      const accounts = await accountApi.fetchAccounts();
      set({ accounts, loading: false, selectedAccountId: null });
    } catch (e) {
      set({ loading: false });
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  selectAccount: (id) => set({ selectedAccountId: id }),

  startOAuth: async (provider) => {
    set({ oauthStatus: "waiting", oauthError: null });
    try {
      const authUrl = await accountApi.startOAuth(provider);
      await openUrl(authUrl);
    } catch (e) {
      set({ oauthStatus: "error", oauthError: errorMessage(e) });
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  startReauth: async (accountId) => {
    set({ oauthStatus: "waiting", oauthError: null, reauthAccountId: accountId });
    try {
      const authUrl = await accountApi.startOAuth("google", accountId);
      await openUrl(authUrl);
    } catch (e) {
      set({ oauthStatus: "error", oauthError: errorMessage(e), reauthAccountId: null });
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  handleOAuthCallback: async (url) => {
    // Prevent double-processing of the same callback
    if (get().oauthStatus === "exchanging") return;
    set({ oauthStatus: "exchanging" });
    try {
      await accountApi.handleOAuthCallback(url);
      const accounts = await accountApi.fetchAccounts();
      set({ accounts, oauthStatus: "success", oauthError: null, reauthAccountId: null });
    } catch (e) {
      set({ oauthStatus: "error", oauthError: errorMessage(e), reauthAccountId: null });
      useErrorStore.getState().addError(errorMessage(e));
    }
  },

  resetOAuth: () => {
    set({ oauthStatus: "idle", oauthError: null, reauthAccountId: null });
  },

  initDeepLinkListener: async () => {
    const unlisten = await listen<string[]>("deep-link://new-url", (event) => {
      const urls = event.payload;
      if (urls.length > 0) {
        const url = urls[0];
        // 部分文字列一致だと https://evil.example/oauth/callback も通るため、
        // スキーム・パスを厳密検証する
        if (isOAuthCallbackUrl(url)) {
          get().handleOAuthCallback(url);
        }
      }
    });
    return unlisten;
  },
}));
