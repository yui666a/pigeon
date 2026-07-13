import { useEffect } from "react";
import type { ReactNode } from "react";

interface ModalProps {
  /** ダイアログの読み上げ名（aria-label） */
  ariaLabel: string;
  /** Escape キー押下時に呼ばれる。オーバーレイクリックでは閉じない */
  onClose: () => void;
  /** パネルに追加する Tailwind クラス（幅・レイアウト等） */
  className?: string;
  children: ReactNode;
}

/**
 * ダイアログ共通のオーバーレイ + パネル。
 * Escape クローズと aria-modal を一元化する（設計上、オーバーレイクリックでは閉じない）。
 */
export function Modal({ ariaLabel, onClose, className, children }: ModalProps) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div
        role="dialog"
        aria-modal="true"
        aria-label={ariaLabel}
        className={`rounded-lg bg-white shadow-xl ${className ?? ""}`}
      >
        {children}
      </div>
    </div>
  );
}
