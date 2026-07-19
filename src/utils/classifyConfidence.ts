import type { Mail } from "../types/mail";

/** 自動割り当ての閾値。これ以上はユーザー確認なしで割り当てる。
 * Rust 側 `classifier::service::CONFIDENCE_AUTO_ASSIGN` と対応する */
export const CONFIDENCE_AUTO_ASSIGN = 0.7;

/** 要確認の下限。これ未満の assign はそもそも永続化されない。
 * Rust 側 `classifier::service::CONFIDENCE_UNCERTAIN` と対応する */
export const CONFIDENCE_UNCERTAIN = 0.4;

/**
 * AI が割り当てたが確信度が中程度で、ユーザーの確認を促すべきメールか。
 *
 * 確信度を持たない AI 割り当て（スレッド追従による機械的な追従）は、
 * 意味的な分類ではないため確認の対象にしない。
 *
 * 設計: docs/design/2026-04-13-phase2-ai-classification-design.md の確信度ゲート
 */
export function needsConfirmation(mail: Mail): boolean {
  if (mail.assigned_by !== "ai") return false;
  const confidence = mail.confidence;
  if (confidence == null) return false;
  return (
    confidence >= CONFIDENCE_UNCERTAIN && confidence < CONFIDENCE_AUTO_ASSIGN
  );
}
