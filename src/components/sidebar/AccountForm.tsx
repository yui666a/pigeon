import { useState, useEffect, useRef } from "react";
import { useAccountStore } from "../../stores/accountStore";
import { ManualAccountForm } from "./ManualAccountForm";
import type { CreateAccountRequest } from "../../types/account";

type FormStep = "select-provider" | "oauth" | "manual";

const OAUTH_TIMEOUT_MS = 10 * 60 * 1000; // 10 minutes

interface AccountFormProps {
  onSubmit: (req: CreateAccountRequest) => void;
  onCancel: () => void;
}

export function AccountForm({ onSubmit, onCancel }: AccountFormProps) {
  const [step, setStep] = useState<FormStep>("select-provider");
  const { oauthStatus, oauthError, startOAuth, resetOAuth } =
    useAccountStore();
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (oauthStatus === "waiting") {
      timeoutRef.current = setTimeout(() => {
        resetOAuth();
        setStep("select-provider");
      }, OAUTH_TIMEOUT_MS);
    }
    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
        timeoutRef.current = null;
      }
    };
  }, [oauthStatus, resetOAuth]);

  // Close form when OAuth completes successfully
  useEffect(() => {
    if (step === "oauth" && oauthStatus === "idle" && !oauthError) {
      // oauth just finished successfully — idle with no error after being in oauth step
    }
  }, [step, oauthStatus, oauthError]);

  const handleGoogleClick = async () => {
    setStep("oauth");
    await startOAuth("google");
  };

  const handleOAuthCancel = () => {
    resetOAuth();
    setStep("select-provider");
  };

  const handleOAuthRetry = () => {
    resetOAuth();
    handleGoogleClick();
  };

  if (step === "manual") {
    return (
      <ManualAccountForm
        onSubmit={onSubmit}
        onBack={() => setStep("select-provider")}
      />
    );
  }

  if (step === "oauth") {
    return (
      <div className="flex flex-col gap-3 p-4">
        <h2 className="text-sm font-semibold">Google アカウント</h2>

        {oauthStatus === "waiting" && (
          <div className="flex flex-col items-center gap-2 py-4">
            <div
              className="h-6 w-6 animate-spin rounded-full border-2 border-blue-600 border-t-transparent"
              role="status"
            />
            <p className="text-sm text-gray-600">ブラウザで認証中です...</p>
            <button
              type="button"
              onClick={handleOAuthCancel}
              className="text-sm text-blue-600 hover:underline"
            >
              キャンセル
            </button>
          </div>
        )}

        {oauthStatus === "exchanging" && (
          <div className="flex flex-col items-center gap-2 py-4">
            <div
              className="h-6 w-6 animate-spin rounded-full border-2 border-blue-600 border-t-transparent"
              role="status"
            />
            <p className="text-sm text-gray-600">アカウントを設定中...</p>
          </div>
        )}

        {oauthStatus === "error" && (
          <div className="flex flex-col gap-2 py-4">
            <p className="text-sm text-red-600">
              {oauthError ?? "認証に失敗しました。"}
            </p>
            <div className="flex gap-2">
              <button
                type="button"
                onClick={handleOAuthRetry}
                className="rounded bg-blue-600 px-4 py-1 text-sm text-white hover:bg-blue-700"
              >
                もう一度試す
              </button>
              <button
                type="button"
                onClick={handleOAuthCancel}
                className="rounded border px-4 py-1 text-sm hover:bg-gray-100"
              >
                戻る
              </button>
            </div>
          </div>
        )}

        {oauthStatus === "idle" && (
          <div className="flex flex-col items-center gap-2 py-4">
            <p className="text-sm text-green-600">
              アカウントを追加しました。
            </p>
          </div>
        )}
      </div>
    );
  }

  // step === "select-provider"
  return (
    <div className="flex flex-col gap-3 p-4">
      <h2 className="text-sm font-semibold">アカウントを追加</h2>
      <button
        type="button"
        onClick={handleGoogleClick}
        className="flex items-center justify-center gap-2 rounded border px-4 py-2 text-sm font-medium hover:bg-gray-50"
      >
        <span className="text-base font-bold text-blue-600">G</span>
        Google でログイン
      </button>
      <button
        type="button"
        onClick={() => setStep("manual")}
        className="rounded border px-4 py-2 text-sm hover:bg-gray-50"
      >
        その他（手動設定）
      </button>
      <button
        type="button"
        onClick={onCancel}
        className="text-sm text-gray-500 hover:underline"
      >
        キャンセル
      </button>
    </div>
  );
}
