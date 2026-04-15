import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Account, CreateAccountRequest, OAuthStatus } from "../types/account";
import { useErrorStore } from "./errorStore";

interface AccountState {
  accounts: Account[];
  selectedAccountId: string | null;
  loading: boolean;
  error: string | null;
  oauthStatus: OAuthStatus;
  oauthError: string | null;
  fetchAccounts: () => Promise<void>;
  createAccount: (req: CreateAccountRequest) => Promise<void>;
  removeAccount: (id: string) => Promise<void>;
  selectAccount: (id: string | null) => void;
  startOAuth: (provider: string) => Promise<void>;
  handleOAuthCallback: (url: string) => Promise<void>;
  resetOAuth: () => void;
  initDeepLinkListener: () => Promise<() => void>;
}

export const useAccountStore = create<AccountState>((set, get) => ({
  accounts: [],
  selectedAccountId: null,
  loading: false,
  error: null,
  oauthStatus: "idle",
  oauthError: null,

  fetchAccounts: async () => {
    set({ loading: true, error: null });
    try {
      const accounts = await invoke<Account[]>("get_accounts");
      set({ accounts, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
      useErrorStore.getState().addError(String(e));
    }
  },

  createAccount: async (req) => {
    set({ loading: true, error: null });
    try {
      await invoke<Account>("create_account", { request: req });
      const accounts = await invoke<Account[]>("get_accounts");
      set({ accounts, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
      useErrorStore.getState().addError(String(e));
    }
  },

  removeAccount: async (id) => {
    set({ loading: true, error: null });
    try {
      await invoke("remove_account", { id });
      const accounts = await invoke<Account[]>("get_accounts");
      set({ accounts, loading: false, selectedAccountId: null });
    } catch (e) {
      set({ error: String(e), loading: false });
      useErrorStore.getState().addError(String(e));
    }
  },

  selectAccount: (id) => set({ selectedAccountId: id }),

  startOAuth: async (provider) => {
    set({ oauthStatus: "waiting", oauthError: null });
    try {
      const authUrl = await invoke<string>("start_oauth", { provider });
      await openUrl(authUrl);
    } catch (e) {
      set({ oauthStatus: "error", oauthError: String(e) });
      useErrorStore.getState().addError(String(e));
    }
  },

  handleOAuthCallback: async (url) => {
    // Prevent double-processing of the same callback
    if (get().oauthStatus === "exchanging") return;
    set({ oauthStatus: "exchanging" });
    try {
      await invoke("handle_oauth_callback", { url });
      const accounts = await invoke<Account[]>("get_accounts");
      set({ accounts, oauthStatus: "idle", oauthError: null });
    } catch (e) {
      set({ oauthStatus: "error", oauthError: String(e) });
      useErrorStore.getState().addError(String(e));
    }
  },

  resetOAuth: () => {
    set({ oauthStatus: "idle", oauthError: null });
  },

  initDeepLinkListener: async () => {
    const unlisten = await listen<string[]>("deep-link://new-url", (event) => {
      const urls = event.payload;
      if (urls.length > 0) {
        const url = urls[0];
        if (url.includes("oauth/callback")) {
          get().handleOAuthCallback(url);
        }
      }
    });
    return unlisten;
  },
}));
