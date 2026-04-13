import { render, screen, fireEvent, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { AccountForm } from "../components/sidebar/AccountForm";
import { useAccountStore } from "../stores/accountStore";

// Mock Tauri APIs
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

describe("AccountForm", () => {
  const mockOnSubmit = vi.fn();
  const mockOnCancel = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    useAccountStore.setState({
      oauthStatus: "idle",
      oauthError: null,
    });
  });

  describe("プロバイダ選択画面", () => {
    it("renders provider selection with Google, manual, and cancel options", () => {
      render(
        <AccountForm onSubmit={mockOnSubmit} onCancel={mockOnCancel} />,
      );
      expect(screen.getByText("アカウントを追加")).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: /Google でログイン/ }),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "その他（手動設定）" }),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "キャンセル" }),
      ).toBeInTheDocument();
    });

    it("calls onCancel when cancel button is clicked", () => {
      render(
        <AccountForm onSubmit={mockOnSubmit} onCancel={mockOnCancel} />,
      );
      fireEvent.click(screen.getByRole("button", { name: "キャンセル" }));
      expect(mockOnCancel).toHaveBeenCalledTimes(1);
    });

    it("shows manual form when 'その他' is clicked", () => {
      render(
        <AccountForm onSubmit={mockOnSubmit} onCancel={mockOnCancel} />,
      );
      fireEvent.click(
        screen.getByRole("button", { name: "その他（手動設定）" }),
      );
      expect(screen.getByLabelText("アカウント名")).toBeInTheDocument();
      expect(screen.getByLabelText("メールアドレス")).toBeInTheDocument();
      expect(screen.getByLabelText("IMAPサーバー")).toBeInTheDocument();
      expect(screen.getByLabelText("パスワード")).toBeInTheDocument();
    });
  });

  describe("OAuth フロー UI", () => {
    it("shows waiting state after clicking Google login", async () => {
      // Mock startOAuth to set the store to waiting (simulating the real behavior)
      useAccountStore.setState({
        startOAuth: vi.fn(async () => {
          useAccountStore.setState({ oauthStatus: "waiting" });
        }),
        resetOAuth: vi.fn(() => {
          useAccountStore.setState({ oauthStatus: "idle", oauthError: null });
        }),
      });

      render(
        <AccountForm onSubmit={mockOnSubmit} onCancel={mockOnCancel} />,
      );
      await act(async () => {
        fireEvent.click(
          screen.getByRole("button", { name: /Google でログイン/ }),
        );
      });
      expect(screen.getByText("ブラウザで認証中です...")).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "キャンセル" }),
      ).toBeInTheDocument();
    });

    it("shows exchanging state", async () => {
      // Mock startOAuth to jump directly to exchanging
      useAccountStore.setState({
        startOAuth: vi.fn(async () => {
          useAccountStore.setState({ oauthStatus: "exchanging" });
        }),
        resetOAuth: vi.fn(() => {
          useAccountStore.setState({ oauthStatus: "idle", oauthError: null });
        }),
      });

      render(
        <AccountForm onSubmit={mockOnSubmit} onCancel={mockOnCancel} />,
      );
      await act(async () => {
        fireEvent.click(
          screen.getByRole("button", { name: /Google でログイン/ }),
        );
      });
      expect(screen.getByText("アカウントを設定中...")).toBeInTheDocument();
    });

    it("shows error state with retry and back buttons", async () => {
      useAccountStore.setState({
        startOAuth: vi.fn(async () => {
          useAccountStore.setState({
            oauthStatus: "error",
            oauthError: "トークン交換に失敗しました",
          });
        }),
        resetOAuth: vi.fn(() => {
          useAccountStore.setState({ oauthStatus: "idle", oauthError: null });
        }),
      });

      render(
        <AccountForm onSubmit={mockOnSubmit} onCancel={mockOnCancel} />,
      );
      await act(async () => {
        fireEvent.click(
          screen.getByRole("button", { name: /Google でログイン/ }),
        );
      });
      expect(
        screen.getByText("トークン交換に失敗しました"),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "もう一度試す" }),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "戻る" }),
      ).toBeInTheDocument();
    });

    it("shows success state when OAuth completes", async () => {
      useAccountStore.setState({
        startOAuth: vi.fn(async () => {
          // Simulate full flow: waiting -> idle (success)
          useAccountStore.setState({ oauthStatus: "idle", oauthError: null });
        }),
        resetOAuth: vi.fn(() => {
          useAccountStore.setState({ oauthStatus: "idle", oauthError: null });
        }),
      });

      render(
        <AccountForm onSubmit={mockOnSubmit} onCancel={mockOnCancel} />,
      );
      await act(async () => {
        fireEvent.click(
          screen.getByRole("button", { name: /Google でログイン/ }),
        );
      });
      expect(
        screen.getByText("アカウントを追加しました。"),
      ).toBeInTheDocument();
    });

    it("returns to provider selection when cancel is clicked during OAuth", async () => {
      const mockResetOAuth = vi.fn(() => {
        useAccountStore.setState({ oauthStatus: "idle", oauthError: null });
      });
      useAccountStore.setState({
        startOAuth: vi.fn(async () => {
          useAccountStore.setState({ oauthStatus: "waiting" });
        }),
        resetOAuth: mockResetOAuth,
      });

      render(
        <AccountForm onSubmit={mockOnSubmit} onCancel={mockOnCancel} />,
      );
      await act(async () => {
        fireEvent.click(
          screen.getByRole("button", { name: /Google でログイン/ }),
        );
      });
      await act(async () => {
        fireEvent.click(
          screen.getByRole("button", { name: "キャンセル" }),
        );
      });
      expect(mockResetOAuth).toHaveBeenCalled();
      expect(screen.getByText("アカウントを追加")).toBeInTheDocument();
    });
  });

  describe("手動設定フォーム", () => {
    it("renders all required input fields in manual mode", () => {
      render(
        <AccountForm onSubmit={mockOnSubmit} onCancel={mockOnCancel} />,
      );
      fireEvent.click(
        screen.getByRole("button", { name: "その他（手動設定）" }),
      );
      expect(screen.getByLabelText("アカウント名")).toBeInTheDocument();
      expect(screen.getByLabelText("メールアドレス")).toBeInTheDocument();
      expect(screen.getByLabelText("IMAPサーバー")).toBeInTheDocument();
      expect(screen.getByLabelText("IMAPポート")).toBeInTheDocument();
      expect(screen.getByLabelText("SMTPサーバー")).toBeInTheDocument();
      expect(screen.getByLabelText("SMTPポート")).toBeInTheDocument();
      expect(screen.getByLabelText("パスワード")).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "追加" }),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "戻る" }),
      ).toBeInTheDocument();
    });

    it("returns to provider selection when back button is clicked", () => {
      render(
        <AccountForm onSubmit={mockOnSubmit} onCancel={mockOnCancel} />,
      );
      fireEvent.click(
        screen.getByRole("button", { name: "その他（手動設定）" }),
      );
      fireEvent.click(screen.getByRole("button", { name: "戻る" }));
      expect(screen.getByText("アカウントを追加")).toBeInTheDocument();
    });
  });
});
