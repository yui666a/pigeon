import { create } from "zustand";

interface ErrorNotification {
  id: string;
  message: string;
  timestamp: number;
}

interface ErrorState {
  errors: ErrorNotification[];
  addError: (message: string) => void;
  dismissError: (id: string) => void;
}

export const useErrorStore = create<ErrorState>((set, get) => ({
  errors: [],
  addError: (message) => {
    const id = crypto.randomUUID();
    set({ errors: [...get().errors, { id, message, timestamp: Date.now() }] });
    // Auto-dismiss after 5 seconds
    setTimeout(() => {
      set({ errors: get().errors.filter((e) => e.id !== id) });
    }, 5000);
  },
  dismissError: (id) => {
    set({ errors: get().errors.filter((e) => e.id !== id) });
  },
}));
