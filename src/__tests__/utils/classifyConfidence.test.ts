import { describe, it, expect } from "vitest";
import {
  CONFIDENCE_AUTO_ASSIGN,
  CONFIDENCE_UNCERTAIN,
  needsConfirmation,
} from "../../utils/classifyConfidence";
import type { Mail } from "../../types/mail";

function makeMail(overrides: Partial<Mail>): Mail {
  return {
    id: "m1",
    account_id: "acc1",
    folder: "INBOX",
    message_id: "<m1@example.com>",
    in_reply_to: null,
    references: null,
    from_addr: "a@example.com",
    to_addr: "b@example.com",
    cc_addr: null,
    subject: "Subject",
    body_text: null,
    body_html: null,
    date: "2026-07-19T10:00:00",
    has_attachments: false,
    raw_size: null,
    uid: 1,
    flags: null,
    is_read: false,
    is_flagged: false,
    fetched_at: "2026-07-19T10:00:00",
    ...overrides,
  };
}

describe("needsConfirmation", () => {
  it("flags a mid-confidence AI assignment", () => {
    expect(
      needsConfirmation(makeMail({ assigned_by: "ai", confidence: 0.55 })),
    ).toBe(true);
  });

  it("includes the lower bound but excludes the auto-assign bound", () => {
    // 閾値の仕様をここに固定する。Rust 側 service.rs の定数と対応
    expect(
      needsConfirmation(
        makeMail({ assigned_by: "ai", confidence: CONFIDENCE_UNCERTAIN }),
      ),
    ).toBe(true);
    expect(
      needsConfirmation(
        makeMail({ assigned_by: "ai", confidence: CONFIDENCE_AUTO_ASSIGN }),
      ),
    ).toBe(false);
  });

  it("does not flag a high-confidence assignment", () => {
    expect(
      needsConfirmation(makeMail({ assigned_by: "ai", confidence: 0.92 })),
    ).toBe(false);
  });

  it("does not flag what the user already confirmed", () => {
    expect(
      needsConfirmation(makeMail({ assigned_by: "user", confidence: 0.55 })),
    ).toBe(false);
  });

  it("does not flag an unassigned mail", () => {
    expect(needsConfirmation(makeMail({}))).toBe(false);
    expect(
      needsConfirmation(makeMail({ assigned_by: null, confidence: null })),
    ).toBe(false);
  });

  it("does not flag an AI assignment that carries no confidence", () => {
    // スレッド追従は assigned_by='ai' / confidence=None で書かれる。
    // 意味的な分類ではないので確認を促さない
    expect(
      needsConfirmation(makeMail({ assigned_by: "ai", confidence: null })),
    ).toBe(false);
  });
});
