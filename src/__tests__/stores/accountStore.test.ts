import { describe, it, expect, vi, beforeEach } from "vitest";
import { useAccountStore } from "../../stores/accountStore";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(() => Promise.resolve()),
}));

describe("accountStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useAccountStore.setState({
      accounts: [],
      selectedAccountId: null,
      loading: false,
      oauthStatus: "idle",
      oauthError: null,
      reauthAccountId: null,
    });
  });

  describe("handleOAuthCallback", () => {
    it("sets oauthStatus to 'success' when the callback succeeds", async () => {
      mockInvoke.mockImplementation((cmd: unknown) => {
        if (cmd === "handle_oauth_callback") return Promise.resolve(null);
        if (cmd === "get_accounts") return Promise.resolve([]);
        return Promise.resolve(null);
      });
      useAccountStore.setState({ oauthStatus: "waiting" });

      await useAccountStore
        .getState()
        .handleOAuthCallback("pigeon://oauth/callback?code=abc");

      expect(useAccountStore.getState().oauthStatus).toBe("success");
      expect(useAccountStore.getState().oauthError).toBeNull();
    });

    it("sets oauthStatus to 'error' when the callback fails", async () => {
      mockInvoke.mockImplementation((cmd: unknown) => {
        if (cmd === "handle_oauth_callback") {
          return Promise.reject("token exchange failed");
        }
        return Promise.resolve([]);
      });
      useAccountStore.setState({ oauthStatus: "waiting" });

      await useAccountStore
        .getState()
        .handleOAuthCallback("pigeon://oauth/callback?code=abc");

      expect(useAccountStore.getState().oauthStatus).toBe("error");
      expect(useAccountStore.getState().oauthError).toContain(
        "token exchange failed",
      );
    });
  });

  describe("resetOAuth", () => {
    it("returns oauthStatus to 'idle' from 'success'", () => {
      useAccountStore.setState({ oauthStatus: "success" });

      useAccountStore.getState().resetOAuth();

      expect(useAccountStore.getState().oauthStatus).toBe("idle");
    });
  });
});
