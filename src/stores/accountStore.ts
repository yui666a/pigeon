import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Account, CreateAccountRequest } from "../types/account";

interface AccountState {
  accounts: Account[];
  selectedAccountId: string | null;
  loading: boolean;
  error: string | null;
  fetchAccounts: () => Promise<void>;
  createAccount: (req: CreateAccountRequest) => Promise<void>;
  removeAccount: (id: string) => Promise<void>;
  selectAccount: (id: string | null) => void;
}

export const useAccountStore = create<AccountState>((set) => ({
  accounts: [],
  selectedAccountId: null,
  loading: false,
  error: null,

  fetchAccounts: async () => {
    set({ loading: true, error: null });
    try {
      const accounts = await invoke<Account[]>("get_accounts");
      set({ accounts, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
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
    }
  },

  selectAccount: (id) => set({ selectedAccountId: id }),
}));
