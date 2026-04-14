# Test Coverage Supplement Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Phase 1-2 の既存コードにユニットテストを補充し、Rust・フロントエンドの両方でカバレッジを底上げする

**Architecture:** 既存のテスト構成（Rust `#[cfg(test)]` inline modules、Vitest + RTL）に追加する形でテストを書く。外部サービス（IMAP、Ollama）に依存するコードはテストスコープ外。純粋ロジックとDB操作に集中する。

**Tech Stack:** Rust (cargo test, rusqlite in-memory), TypeScript (Vitest, React Testing Library, Zustand)

---

## File Structure

### Rust — 既存ファイルの `#[cfg(test)]` モジュールにテストを追加

| File | Action | What |
|------|--------|------|
| `src-tauri/src/models/classifier.rs` | Modify | `MailSummary::from_mail` のテスト追加 |
| `src-tauri/src/models/account.rs` | Modify | `AccountProvider`, `AuthType` の変換テスト追加 |
| `src-tauri/src/classifier/prompt.rs` | Modify | エッジケーステスト追加 |
| `src-tauri/src/classifier/ollama.rs` | Modify | JSON パースのエッジケーステスト追加 |
| `src-tauri/src/db/mails.rs` | Modify | スレッディングのエッジケーステスト追加 |
| `src-tauri/src/mail_sync/mime_parser.rs` | Modify | MIME パースのエッジケーステスト追加 |

### Frontend — 新規テストファイル作成

| File | Action | What |
|------|--------|------|
| `src/__tests__/stores/projectStore.test.ts` | Create | projectStore の状態管理テスト |
| `src/__tests__/stores/mailStore.test.ts` | Create | mailStore の状態管理テスト |
| `src/__tests__/stores/classifyStore.test.ts` | Create | classifyStore の状態管理テスト |
| `src/__tests__/ClassifyResultBadge.test.tsx` | Create | 信頼度バッジの表示テスト |
| `src/__tests__/NewProjectProposal.test.tsx` | Create | 新規案件提案フォームのテスト |
| `src/__tests__/ThreadItem.test.tsx` | Create | スレッド行の表示テスト |
| `src/__tests__/MailHeader.test.tsx` | Create | メールヘッダーの表示テスト |
| `src/__tests__/ProjectForm.test.tsx` | Create | 案件フォームのテスト |
| `src/__tests__/ClassifyButton.test.tsx` | Create | 分類ボタン・プログレスバーのテスト |

---

## Task 1: Rust — `MailSummary::from_mail` テスト

**Files:**
- Modify: `src-tauri/src/models/classifier.rs:14-30`

- [ ] **Step 1: Write the failing tests**

`src-tauri/src/models/classifier.rs` の末尾（ファイル末尾）に `#[cfg(test)]` モジュールを追加:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::mail::Mail;

    fn make_mail(body_text: Option<&str>) -> Mail {
        Mail {
            id: "m1".into(),
            account_id: "acc1".into(),
            folder: "INBOX".into(),
            message_id: "<msg1@example.com>".into(),
            in_reply_to: None,
            references: None,
            from_addr: "sender@example.com".into(),
            to_addr: "me@example.com".into(),
            cc_addr: None,
            subject: "Test Subject".into(),
            body_text: body_text.map(|s| s.to_string()),
            body_html: None,
            date: "2026-04-13T10:00:00".into(),
            has_attachments: false,
            raw_size: None,
            uid: 1,
            flags: None,
            fetched_at: "2026-04-13T00:00:00".into(),
        }
    }

    #[test]
    fn test_from_mail_basic() {
        let mail = make_mail(Some("Hello, this is a short body."));
        let summary = MailSummary::from_mail(&mail);
        assert_eq!(summary.subject, "Test Subject");
        assert_eq!(summary.from_addr, "sender@example.com");
        assert_eq!(summary.date, "2026-04-13T10:00:00");
        assert_eq!(summary.body_preview, "Hello, this is a short body.");
    }

    #[test]
    fn test_from_mail_truncates_body_at_300_chars() {
        let long_body = "a".repeat(500);
        let mail = make_mail(Some(&long_body));
        let summary = MailSummary::from_mail(&mail);
        assert_eq!(summary.body_preview.len(), 300);
    }

    #[test]
    fn test_from_mail_empty_body() {
        let mail = make_mail(None);
        let summary = MailSummary::from_mail(&mail);
        assert_eq!(summary.body_preview, "");
    }

    #[test]
    fn test_from_mail_multibyte_truncation() {
        // 日本語300文字 — chars() で切るのでパニックしないことを確認
        let japanese_body = "あ".repeat(500);
        let mail = make_mail(Some(&japanese_body));
        let summary = MailSummary::from_mail(&mail);
        assert_eq!(summary.body_preview.chars().count(), 300);
    }
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cd src-tauri && cargo test models::classifier::tests -v`
Expected: 4 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/models/classifier.rs
git commit -m "test(models): MailSummary::from_mail のユニットテスト追加"
```

---

## Task 2: Rust — `AccountProvider` / `AuthType` 変換テスト

**Files:**
- Modify: `src-tauri/src/models/account.rs`

- [ ] **Step 1: Write the tests**

`src-tauri/src/models/account.rs` の末尾に `#[cfg(test)]` モジュールを追加:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_provider_supports_oauth() {
        assert!(AccountProvider::Google.supports_oauth());
        assert!(!AccountProvider::Other.supports_oauth());
    }

    #[test]
    fn test_account_provider_as_str() {
        assert_eq!(AccountProvider::Google.as_str(), "google");
        assert_eq!(AccountProvider::Other.as_str(), "other");
    }

    #[test]
    fn test_account_provider_try_from_valid() {
        assert_eq!(AccountProvider::try_from("google").unwrap(), AccountProvider::Google);
        assert_eq!(AccountProvider::try_from("other").unwrap(), AccountProvider::Other);
    }

    #[test]
    fn test_account_provider_try_from_invalid() {
        assert!(AccountProvider::try_from("yahoo").is_err());
        assert!(AccountProvider::try_from("").is_err());
    }

    #[test]
    fn test_auth_type_as_str() {
        assert_eq!(AuthType::Plain.as_str(), "plain");
        assert_eq!(AuthType::Oauth2.as_str(), "oauth2");
    }

    #[test]
    fn test_auth_type_try_from_valid() {
        assert!(matches!(AuthType::try_from("plain").unwrap(), AuthType::Plain));
        assert!(matches!(AuthType::try_from("oauth2").unwrap(), AuthType::Oauth2));
    }

    #[test]
    fn test_auth_type_try_from_invalid() {
        assert!(AuthType::try_from("basic").is_err());
        assert!(AuthType::try_from("PLAIN").is_err());
    }

    #[test]
    fn test_account_provider_roundtrip() {
        for provider in [AccountProvider::Google, AccountProvider::Other] {
            let s = provider.as_str();
            let back = AccountProvider::try_from(s).unwrap();
            assert_eq!(back, provider);
        }
    }
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cd src-tauri && cargo test models::account::tests -v`
Expected: 8 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/models/account.rs
git commit -m "test(models): AccountProvider/AuthType の変換テスト追加"
```

---

## Task 3: Rust — プロンプト構築のエッジケーステスト

**Files:**
- Modify: `src-tauri/src/classifier/prompt.rs:80-162`

- [ ] **Step 1: Add edge case tests**

既存の `#[cfg(test)] mod tests` ブロック内に以下を追加:

```rust
    #[test]
    fn test_build_user_prompt_project_without_description() {
        let mail = make_mail();
        let projects = vec![ProjectSummary {
            id: "p1".to_string(),
            name: "No Desc Project".to_string(),
            description: None,
            recent_subjects: vec![],
        }];
        let prompt = build_user_prompt(&mail, &projects, &[]);

        assert!(prompt.contains("p1"));
        assert!(prompt.contains("No Desc Project"));
        assert!(!prompt.contains("description:"));
    }

    #[test]
    fn test_build_user_prompt_project_without_recent_subjects() {
        let mail = make_mail();
        let projects = vec![ProjectSummary {
            id: "p1".to_string(),
            name: "Empty Project".to_string(),
            description: Some("desc".to_string()),
            recent_subjects: vec![],
        }];
        let prompt = build_user_prompt(&mail, &projects, &[]);

        assert!(!prompt.contains("Recent subjects"));
    }

    #[test]
    fn test_build_user_prompt_many_corrections() {
        let mail = make_mail();
        let corrections: Vec<CorrectionEntry> = (0..5)
            .map(|i| CorrectionEntry {
                mail_subject: format!("Mail {}", i),
                from_project: Some(format!("proj-{}", i)),
                to_project: format!("proj-{}", i + 1),
            })
            .collect();
        let prompt = build_user_prompt(&mail, &[], &corrections);

        assert!(prompt.contains("Past corrections"));
        for i in 0..5 {
            assert!(prompt.contains(&format!("Mail {}", i)));
        }
    }

    #[test]
    fn test_build_user_prompt_contains_all_mail_fields() {
        let mail = MailSummary {
            subject: "特殊文字テスト <>&\"'".to_string(),
            from_addr: "日本語名前 <test@example.com>".to_string(),
            date: "2026-04-13".to_string(),
            body_preview: "本文プレビュー".to_string(),
        };
        let prompt = build_user_prompt(&mail, &[], &[]);

        assert!(prompt.contains("特殊文字テスト <>&\"'"));
        assert!(prompt.contains("日本語名前 <test@example.com>"));
        assert!(prompt.contains("本文プレビュー"));
    }
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cd src-tauri && cargo test classifier::prompt::tests -v`
Expected: 7 tests PASS (3 existing + 4 new)

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/classifier/prompt.rs
git commit -m "test(classifier): プロンプト構築のエッジケーステスト追加"
```

---

## Task 4: Rust — Ollama JSON パースのエッジケーステスト

**Files:**
- Modify: `src-tauri/src/classifier/ollama.rs:160-241`

- [ ] **Step 1: Add edge case tests**

既存の `#[cfg(test)] mod tests` ブロック内に以下を追加:

```rust
    #[test]
    fn test_extract_json_empty_string() {
        assert!(OllamaClassifier::extract_json("").is_none());
    }

    #[test]
    fn test_extract_json_only_open_brace() {
        // rfind('}') returns None
        assert!(OllamaClassifier::extract_json("{").is_none());
    }

    #[test]
    fn test_extract_json_only_close_brace() {
        // find('{') returns None
        assert!(OllamaClassifier::extract_json("}").is_none());
    }

    #[test]
    fn test_extract_json_nested_braces() {
        let input = r#"{"outer": {"inner": "value"}}"#;
        let result = OllamaClassifier::extract_json(input).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn test_parse_response_missing_confidence() {
        let content = r#"{"action": "unclassified", "reason": "test"}"#;
        let result = OllamaClassifier::parse_response(content);
        // confidence is required, so this should fail
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_response_missing_reason() {
        let content = r#"{"action": "unclassified", "confidence": 0.5}"#;
        let result = OllamaClassifier::parse_response(content);
        // reason is required, so this should fail
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_response_unknown_action() {
        let content = r#"{"action": "delete", "confidence": 0.5, "reason": "test"}"#;
        let result = OllamaClassifier::parse_response(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_response_assign_missing_project_id() {
        let content = r#"{"action": "assign", "confidence": 0.9, "reason": "test"}"#;
        let result = OllamaClassifier::parse_response(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_response_create_missing_fields() {
        let content = r#"{"action": "create", "confidence": 0.7, "reason": "test"}"#;
        let result = OllamaClassifier::parse_response(content);
        // project_name and description are required for create
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_response_confidence_boundary_values() {
        let content = r#"{"action": "unclassified", "confidence": 0.0, "reason": "test"}"#;
        let result = OllamaClassifier::parse_response(content).unwrap();
        assert!((result.confidence - 0.0).abs() < f64::EPSILON);

        let content = r#"{"action": "unclassified", "confidence": 1.0, "reason": "test"}"#;
        let result = OllamaClassifier::parse_response(content).unwrap();
        assert!((result.confidence - 1.0).abs() < f64::EPSILON);
    }
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cd src-tauri && cargo test classifier::ollama::tests -v`
Expected: 17 tests PASS (8 existing + 9 new)

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/classifier/ollama.rs
git commit -m "test(classifier): Ollama JSONパースのエッジケーステスト追加"
```

---

## Task 5: Rust — スレッディングのエッジケーステスト

**Files:**
- Modify: `src-tauri/src/db/mails.rs:204-319`

- [ ] **Step 1: Add edge case tests**

既存の `#[cfg(test)] mod tests` ブロック内に以下を追加:

```rust
    #[test]
    fn test_build_threads_empty() {
        let threads = build_threads(&[]);
        assert!(threads.is_empty());
    }

    #[test]
    fn test_build_threads_single_mail() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Solo", "2026-04-13T10:00:00");
        let threads = build_threads(&[m1]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 1);
        assert_eq!(threads[0].subject, "Solo");
    }

    #[test]
    fn test_build_threads_sorted_by_last_date_desc() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Old Topic", "2026-04-10T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "New Topic", "2026-04-13T10:00:00");
        let threads = build_threads(&[m1, m2]);
        assert_eq!(threads.len(), 2);
        assert_eq!(threads[0].subject, "New Topic");
        assert_eq!(threads[1].subject, "Old Topic");
    }

    #[test]
    fn test_build_threads_fw_prefix_groups() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "案件の件", "2026-04-13T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "Fw: 案件の件", "2026-04-13T11:00:00");
        let threads = build_threads(&[m1, m2]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 2);
    }

    #[test]
    fn test_build_threads_fwd_prefix_groups() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Report", "2026-04-13T10:00:00");
        let m2 = make_mail("m2", "<msg2@ex.com>", "Fwd: Report", "2026-04-13T11:00:00");
        let threads = build_threads(&[m1, m2]);
        assert_eq!(threads.len(), 1);
    }

    #[test]
    fn test_build_threads_deep_chain() {
        let m1 = make_mail("m1", "<msg1@ex.com>", "Topic", "2026-04-13T10:00:00");
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Re: Topic", "2026-04-13T11:00:00");
        m2.in_reply_to = Some("<msg1@ex.com>".into());
        let mut m3 = make_mail("m3", "<msg3@ex.com>", "Re: Re: Topic", "2026-04-13T12:00:00");
        m3.in_reply_to = Some("<msg2@ex.com>".into());
        let mut m4 = make_mail("m4", "<msg4@ex.com>", "Re: Re: Re: Topic", "2026-04-13T13:00:00");
        m4.in_reply_to = Some("<msg3@ex.com>".into());
        let threads = build_threads(&[m1, m2, m3, m4]);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].mail_count, 4);
    }

    #[test]
    fn test_build_threads_from_addrs_deduplication() {
        let mut m1 = make_mail("m1", "<msg1@ex.com>", "Topic", "2026-04-13T10:00:00");
        m1.from_addr = "alice@example.com".into();
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Re: Topic", "2026-04-13T11:00:00");
        m2.from_addr = "alice@example.com".into();
        m2.in_reply_to = Some("<msg1@ex.com>".into());
        let threads = build_threads(&[m1, m2]);
        assert_eq!(threads[0].from_addrs.len(), 1);
    }

    #[test]
    fn test_build_threads_subject_grouping_skipped_when_has_references() {
        // m2 has references pointing to a non-existent message, so subject fallback should NOT apply
        let m1 = make_mail("m1", "<msg1@ex.com>", "Same Subject", "2026-04-13T10:00:00");
        let mut m2 = make_mail("m2", "<msg2@ex.com>", "Same Subject", "2026-04-13T11:00:00");
        m2.references = Some("<nonexistent@ex.com>".into());
        let threads = build_threads(&[m1, m2]);
        // m2 has references set, so subject fallback is skipped — separate threads
        assert_eq!(threads.len(), 2);
    }

    #[test]
    fn test_normalize_subject_nested_prefixes() {
        assert_eq!(normalize_subject("Re: Fw: Re: Hello"), "hello");
        assert_eq!(normalize_subject("FW: FWD: RE: Hello"), "hello");
    }

    #[test]
    fn test_normalize_subject_case_insensitive() {
        assert_eq!(normalize_subject("RE: HELLO"), "hello");
        assert_eq!(normalize_subject("re: hello"), "hello");
    }

    #[test]
    fn test_normalize_subject_whitespace() {
        assert_eq!(normalize_subject("  Re:   Hello  "), "hello");
    }
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cd src-tauri && cargo test db::mails::tests -v`
Expected: 17 tests PASS (7 existing + 10 new)

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/db/mails.rs
git commit -m "test(db): スレッディング・normalize_subject のエッジケーステスト追加"
```

---

## Task 6: Rust — MIME パーサーのエッジケーステスト

**Files:**
- Modify: `src-tauri/src/mail_sync/mime_parser.rs:91-138`

- [ ] **Step 1: Add edge case tests**

既存の `#[cfg(test)] mod tests` ブロック内に以下を追加:

```rust
    const EMAIL_WITH_CC: &[u8] = b"From: sender@example.com\r\n\
        To: recipient@example.com\r\n\
        Cc: cc1@example.com, cc2@example.com\r\n\
        Subject: CC Test\r\n\
        Message-ID: <cc-test@example.com>\r\n\
        Date: Mon, 13 Apr 2026 10:00:00 +0900\r\n\
        \r\n\
        Body with CC.";

    const EMAIL_NO_SUBJECT: &[u8] = b"From: sender@example.com\r\n\
        To: recipient@example.com\r\n\
        Message-ID: <nosub@example.com>\r\n\
        Date: Mon, 13 Apr 2026 10:00:00 +0900\r\n\
        \r\n\
        Body without subject.";

    const EMAIL_WITH_DISPLAY_NAME: &[u8] = b"From: Alice Smith <alice@example.com>\r\n\
        To: Bob Jones <bob@example.com>\r\n\
        Subject: Display Name Test\r\n\
        Message-ID: <display@example.com>\r\n\
        Date: Mon, 13 Apr 2026 10:00:00 +0900\r\n\
        \r\n\
        Hello Bob.";

    const EMAIL_WITH_REFERENCES_CHAIN: &[u8] = b"From: sender@example.com\r\n\
        To: recipient@example.com\r\n\
        Subject: Re: Re: Chain\r\n\
        Message-ID: <chain3@example.com>\r\n\
        In-Reply-To: <chain2@example.com>\r\n\
        References: <chain1@example.com> <chain2@example.com>\r\n\
        Date: Mon, 13 Apr 2026 12:00:00 +0900\r\n\
        \r\n\
        Third reply.";

    #[test]
    fn test_parse_email_with_cc() {
        let mail = parse_mime(EMAIL_WITH_CC, "acc1", "INBOX", 3).unwrap();
        assert!(mail.cc_addr.is_some());
        let cc = mail.cc_addr.unwrap();
        assert!(cc.contains("cc1@example.com"));
        assert!(cc.contains("cc2@example.com"));
    }

    #[test]
    fn test_parse_email_no_subject_defaults() {
        let mail = parse_mime(EMAIL_NO_SUBJECT, "acc1", "INBOX", 4).unwrap();
        assert_eq!(mail.subject, "(no subject)");
    }

    #[test]
    fn test_parse_email_with_display_name() {
        let mail = parse_mime(EMAIL_WITH_DISPLAY_NAME, "acc1", "INBOX", 5).unwrap();
        assert!(mail.from_addr.contains("Alice Smith"));
        assert!(mail.from_addr.contains("alice@example.com"));
    }

    #[test]
    fn test_parse_email_with_references_chain() {
        let mail = parse_mime(EMAIL_WITH_REFERENCES_CHAIN, "acc1", "INBOX", 6).unwrap();
        assert_eq!(mail.in_reply_to, Some("<chain2@example.com>".to_string()));
        let refs = mail.references.unwrap();
        assert!(refs.contains("<chain1@example.com>"));
        assert!(refs.contains("<chain2@example.com>"));
    }

    #[test]
    fn test_parse_email_sets_account_and_folder() {
        let mail = parse_mime(SIMPLE_EMAIL, "my-account", "Sent", 10).unwrap();
        assert_eq!(mail.account_id, "my-account");
        assert_eq!(mail.folder, "Sent");
        assert_eq!(mail.uid, 10);
    }

    #[test]
    fn test_parse_email_no_attachments() {
        let mail = parse_mime(SIMPLE_EMAIL, "acc1", "INBOX", 1).unwrap();
        assert!(!mail.has_attachments);
    }

    #[test]
    fn test_parse_email_raw_size() {
        let mail = parse_mime(SIMPLE_EMAIL, "acc1", "INBOX", 1).unwrap();
        assert_eq!(mail.raw_size, Some(SIMPLE_EMAIL.len() as i64));
    }

    #[test]
    fn test_parse_empty_bytes() {
        let result = parse_mime(b"", "acc1", "INBOX", 1);
        // Should either return None or a partial Mail — must not panic
        let _ = result;
    }
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cd src-tauri && cargo test mail_sync::mime_parser::tests -v`
Expected: 11 tests PASS (3 existing + 8 new)

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/mail_sync/mime_parser.rs
git commit -m "test(mime): MIMEパーサーのエッジケーステスト追加"
```

---

## Task 7: Frontend — ClassifyResultBadge テスト

**Files:**
- Create: `src/__tests__/ClassifyResultBadge.test.tsx`

- [ ] **Step 1: Write the test file**

```tsx
import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { ClassifyResultBadge } from "../components/common/ClassifyResultBadge";

describe("ClassifyResultBadge", () => {
  it("returns null for user-assigned mails", () => {
    const { container } = render(
      <ClassifyResultBadge confidence={0.95} assignedBy="user" />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("shows green AI badge for high confidence (>= 0.7)", () => {
    render(<ClassifyResultBadge confidence={0.85} assignedBy="ai" />);
    const badge = screen.getByText("AI");
    expect(badge).toBeInTheDocument();
    expect(badge.className).toContain("bg-green-100");
  });

  it("shows yellow warning AI badge for uncertain confidence (0.4-0.7)", () => {
    render(<ClassifyResultBadge confidence={0.55} assignedBy="ai" />);
    const badge = screen.getByText("AI");
    expect(badge).toBeInTheDocument();
    expect(badge.className).toContain("bg-yellow-100");
  });

  it("returns null for low confidence (< 0.4)", () => {
    const { container } = render(
      <ClassifyResultBadge confidence={0.2} assignedBy="ai" />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("shows green badge at exactly 0.7 boundary", () => {
    render(<ClassifyResultBadge confidence={0.7} assignedBy="ai" />);
    const badge = screen.getByText("AI");
    expect(badge.className).toContain("bg-green-100");
  });

  it("shows yellow badge at exactly 0.4 boundary", () => {
    render(<ClassifyResultBadge confidence={0.4} assignedBy="ai" />);
    const badge = screen.getByText("AI");
    expect(badge.className).toContain("bg-yellow-100");
  });
});
```

- [ ] **Step 2: Run test to verify it passes**

Run: `pnpm test -- --run src/__tests__/ClassifyResultBadge.test.tsx`
Expected: 6 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/__tests__/ClassifyResultBadge.test.tsx
git commit -m "test(ui): ClassifyResultBadge の信頼度別表示テスト追加"
```

---

## Task 8: Frontend — NewProjectProposal テスト

**Files:**
- Create: `src/__tests__/NewProjectProposal.test.tsx`

- [ ] **Step 1: Write the test file**

```tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { NewProjectProposal } from "../components/common/NewProjectProposal";

describe("NewProjectProposal", () => {
  const defaultProps = {
    mailId: "mail-1",
    suggestedName: "新規案件",
    suggestedDescription: "AIが提案した説明",
    reason: "既存プロジェクトに一致なし",
    onApprove: vi.fn(),
    onReject: vi.fn(),
  };

  it("renders reason text and pre-filled form", () => {
    render(<NewProjectProposal {...defaultProps} />);
    expect(screen.getByText("既存プロジェクトに一致なし")).toBeInTheDocument();
    expect(screen.getByDisplayValue("新規案件")).toBeInTheDocument();
    expect(screen.getByDisplayValue("AIが提案した説明")).toBeInTheDocument();
  });

  it("calls onApprove with edited name and description", () => {
    const onApprove = vi.fn();
    render(<NewProjectProposal {...defaultProps} onApprove={onApprove} />);

    const nameInput = screen.getByDisplayValue("新規案件");
    fireEvent.change(nameInput, { target: { value: "修正された案件名" } });

    fireEvent.click(screen.getByText("案件を作成"));
    expect(onApprove).toHaveBeenCalledWith(
      "mail-1",
      "修正された案件名",
      "AIが提案した説明",
    );
  });

  it("calls onReject with mailId", () => {
    const onReject = vi.fn();
    render(<NewProjectProposal {...defaultProps} onReject={onReject} />);

    fireEvent.click(screen.getByText("却下"));
    expect(onReject).toHaveBeenCalledWith("mail-1");
  });

  it("disables approve button when name is empty", () => {
    render(<NewProjectProposal {...defaultProps} />);
    const nameInput = screen.getByDisplayValue("新規案件");
    fireEvent.change(nameInput, { target: { value: "" } });

    const button = screen.getByText("案件を作成");
    expect(button).toBeDisabled();
  });

  it("disables approve button when name is whitespace only", () => {
    render(<NewProjectProposal {...defaultProps} />);
    const nameInput = screen.getByDisplayValue("新規案件");
    fireEvent.change(nameInput, { target: { value: "   " } });

    const button = screen.getByText("案件を作成");
    expect(button).toBeDisabled();
  });

  it("calls onApprove with undefined description when empty", () => {
    const onApprove = vi.fn();
    render(
      <NewProjectProposal
        {...defaultProps}
        suggestedDescription={undefined}
        onApprove={onApprove}
      />,
    );

    fireEvent.click(screen.getByText("案件を作成"));
    expect(onApprove).toHaveBeenCalledWith("mail-1", "新規案件", undefined);
  });
});
```

- [ ] **Step 2: Run test to verify it passes**

Run: `pnpm test -- --run src/__tests__/NewProjectProposal.test.tsx`
Expected: 6 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/__tests__/NewProjectProposal.test.tsx
git commit -m "test(ui): NewProjectProposal フォームのテスト追加"
```

---

## Task 9: Frontend — ThreadItem テスト

**Files:**
- Create: `src/__tests__/ThreadItem.test.tsx`

- [ ] **Step 1: Write the test file**

```tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ThreadItem } from "../components/thread-list/ThreadItem";
import type { Thread } from "../types/mail";

function makeThread(overrides: Partial<Thread> = {}): Thread {
  return {
    thread_id: "<thread-1@example.com>",
    subject: "テストスレッド",
    last_date: "2026-04-13T10:00:00+09:00",
    mail_count: 1,
    from_addrs: ["alice@example.com"],
    mails: [],
    ...overrides,
  };
}

describe("ThreadItem", () => {
  it("renders subject and date", () => {
    render(
      <ThreadItem
        thread={makeThread()}
        selected={false}
        onClick={vi.fn()}
      />,
    );
    expect(screen.getByText("テストスレッド")).toBeInTheDocument();
    // Date is formatted as M/D
    expect(screen.getByText("4/13")).toBeInTheDocument();
  });

  it("renders from addresses", () => {
    render(
      <ThreadItem
        thread={makeThread({ from_addrs: ["alice@example.com", "bob@example.com"] })}
        selected={false}
        onClick={vi.fn()}
      />,
    );
    expect(screen.getByText("alice@example.com, bob@example.com")).toBeInTheDocument();
  });

  it("shows mail count badge when > 1", () => {
    render(
      <ThreadItem
        thread={makeThread({ mail_count: 5 })}
        selected={false}
        onClick={vi.fn()}
      />,
    );
    expect(screen.getByText("5")).toBeInTheDocument();
  });

  it("hides mail count badge for single mail", () => {
    render(
      <ThreadItem
        thread={makeThread({ mail_count: 1 })}
        selected={false}
        onClick={vi.fn()}
      />,
    );
    expect(screen.queryByText("1")).not.toBeInTheDocument();
  });

  it("applies selected style", () => {
    render(
      <ThreadItem
        thread={makeThread()}
        selected={true}
        onClick={vi.fn()}
      />,
    );
    const button = screen.getByRole("button");
    expect(button.className).toContain("bg-blue-50");
  });

  it("calls onClick when clicked", () => {
    const onClick = vi.fn();
    render(
      <ThreadItem
        thread={makeThread()}
        selected={false}
        onClick={onClick}
      />,
    );
    fireEvent.click(screen.getByRole("button"));
    expect(onClick).toHaveBeenCalledTimes(1);
  });
});
```

- [ ] **Step 2: Run test to verify it passes**

Run: `pnpm test -- --run src/__tests__/ThreadItem.test.tsx`
Expected: 6 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/__tests__/ThreadItem.test.tsx
git commit -m "test(ui): ThreadItem のレンダリングテスト追加"
```

---

## Task 10: Frontend — MailHeader テスト

**Files:**
- Create: `src/__tests__/MailHeader.test.tsx`

- [ ] **Step 1: Write the test file**

```tsx
import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { MailHeader } from "../components/mail-view/MailHeader";
import type { Mail } from "../types/mail";

function makeMail(overrides: Partial<Mail> = {}): Mail {
  return {
    id: "m1",
    account_id: "acc1",
    folder: "INBOX",
    message_id: "<msg1@example.com>",
    in_reply_to: null,
    references: null,
    from_addr: "Alice <alice@example.com>",
    to_addr: "bob@example.com",
    cc_addr: null,
    subject: "テストメール件名",
    body_text: "本文",
    body_html: null,
    date: "2026-04-13T10:00:00+09:00",
    has_attachments: false,
    raw_size: null,
    uid: 1,
    flags: null,
    fetched_at: "2026-04-13T00:00:00",
    ...overrides,
  };
}

describe("MailHeader", () => {
  it("renders subject", () => {
    render(<MailHeader mail={makeMail()} />);
    expect(screen.getByText("テストメール件名")).toBeInTheDocument();
  });

  it("renders from address", () => {
    render(<MailHeader mail={makeMail()} />);
    expect(screen.getByText("Alice <alice@example.com>")).toBeInTheDocument();
  });

  it("renders to address", () => {
    render(<MailHeader mail={makeMail()} />);
    expect(screen.getByText("bob@example.com")).toBeInTheDocument();
  });

  it("renders cc when present", () => {
    render(
      <MailHeader mail={makeMail({ cc_addr: "cc@example.com" })} />,
    );
    expect(screen.getByText("cc@example.com")).toBeInTheDocument();
    expect(screen.getByText("Cc:")).toBeInTheDocument();
  });

  it("hides cc when null", () => {
    render(<MailHeader mail={makeMail({ cc_addr: null })} />);
    expect(screen.queryByText("Cc:")).not.toBeInTheDocument();
  });

  it("renders formatted date", () => {
    render(<MailHeader mail={makeMail()} />);
    // Date: ラベルが存在することを確認
    expect(screen.getByText("Date:")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it passes**

Run: `pnpm test -- --run src/__tests__/MailHeader.test.tsx`
Expected: 6 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/__tests__/MailHeader.test.tsx
git commit -m "test(ui): MailHeader のレンダリングテスト追加"
```

---

## Task 11: Frontend — ProjectForm テスト

**Files:**
- Create: `src/__tests__/ProjectForm.test.tsx`

- [ ] **Step 1: Write the test file**

```tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ProjectForm } from "../components/sidebar/ProjectForm";

describe("ProjectForm", () => {
  const mockOnSubmit = vi.fn();
  const mockOnCancel = vi.fn();

  it("renders all form fields", () => {
    render(<ProjectForm onSubmit={mockOnSubmit} onCancel={mockOnCancel} />);
    expect(screen.getByPlaceholderText("案件名を入力")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("説明（任意）")).toBeInTheDocument();
    expect(screen.getByText("作成")).toBeInTheDocument();
    expect(screen.getByText("キャンセル")).toBeInTheDocument();
  });

  it("calls onSubmit with trimmed name and description", () => {
    render(<ProjectForm onSubmit={mockOnSubmit} onCancel={mockOnCancel} />);

    fireEvent.change(screen.getByPlaceholderText("案件名を入力"), {
      target: { value: "  新しい案件  " },
    });
    fireEvent.change(screen.getByPlaceholderText("説明（任意）"), {
      target: { value: "案件の説明" },
    });
    fireEvent.click(screen.getByText("作成"));

    expect(mockOnSubmit).toHaveBeenCalledWith(
      "新しい案件",
      "案件の説明",
      "#6b7280", // default color
    );
  });

  it("does not submit with empty name", () => {
    render(<ProjectForm onSubmit={mockOnSubmit} onCancel={mockOnCancel} />);

    fireEvent.click(screen.getByText("作成"));
    expect(mockOnSubmit).not.toHaveBeenCalled();
  });

  it("passes undefined description when empty", () => {
    const onSubmit = vi.fn();
    render(<ProjectForm onSubmit={onSubmit} onCancel={mockOnCancel} />);

    fireEvent.change(screen.getByPlaceholderText("案件名を入力"), {
      target: { value: "案件名" },
    });
    // description is left empty
    fireEvent.click(screen.getByText("作成"));

    expect(onSubmit).toHaveBeenCalledWith("案件名", undefined, "#6b7280");
  });

  it("calls onCancel when cancel button is clicked", () => {
    const onCancel = vi.fn();
    render(<ProjectForm onSubmit={mockOnSubmit} onCancel={onCancel} />);

    fireEvent.click(screen.getByText("キャンセル"));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});
```

- [ ] **Step 2: Run test to verify it passes**

Run: `pnpm test -- --run src/__tests__/ProjectForm.test.tsx`
Expected: 5 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/__tests__/ProjectForm.test.tsx
git commit -m "test(ui): ProjectForm のフォーム操作テスト追加"
```

---

## Task 12: Frontend — ClassifyButton テスト

**Files:**
- Create: `src/__tests__/ClassifyButton.test.tsx`

- [ ] **Step 1: Write the test file**

```tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ClassifyButton } from "../components/thread-list/ClassifyButton";
import { useClassifyStore } from "../stores/classifyStore";

// Mock Tauri APIs
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

describe("ClassifyButton", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows classify button in idle state", () => {
    useClassifyStore.setState({
      classifying: false,
      progress: null,
      classifyAll: vi.fn(),
      cancelClassification: vi.fn(),
    });

    render(<ClassifyButton accountId="acc1" />);
    expect(screen.getByText("分類する")).toBeInTheDocument();
  });

  it("calls classifyAll when button is clicked", () => {
    const classifyAll = vi.fn();
    useClassifyStore.setState({
      classifying: false,
      progress: null,
      classifyAll,
      cancelClassification: vi.fn(),
    });

    render(<ClassifyButton accountId="acc1" />);
    fireEvent.click(screen.getByText("分類する"));
    expect(classifyAll).toHaveBeenCalledWith("acc1");
  });

  it("shows progress bar when classifying", () => {
    useClassifyStore.setState({
      classifying: true,
      progress: { current: 3, total: 10 },
      classifyAll: vi.fn(),
      cancelClassification: vi.fn(),
    });

    render(<ClassifyButton accountId="acc1" />);
    expect(screen.getByText("3 / 10")).toBeInTheDocument();
    expect(screen.getByText("キャンセル")).toBeInTheDocument();
  });

  it("calls cancelClassification when cancel is clicked", () => {
    const cancelClassification = vi.fn();
    useClassifyStore.setState({
      classifying: true,
      progress: { current: 1, total: 5 },
      classifyAll: vi.fn(),
      cancelClassification,
    });

    render(<ClassifyButton accountId="acc1" />);
    fireEvent.click(screen.getByText("キャンセル"));
    expect(cancelClassification).toHaveBeenCalledTimes(1);
  });

  it("shows progress bar without text when progress is null during classifying", () => {
    useClassifyStore.setState({
      classifying: true,
      progress: null,
      classifyAll: vi.fn(),
      cancelClassification: vi.fn(),
    });

    render(<ClassifyButton accountId="acc1" />);
    expect(screen.getByText("キャンセル")).toBeInTheDocument();
    expect(screen.queryByText("/")).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it passes**

Run: `pnpm test -- --run src/__tests__/ClassifyButton.test.tsx`
Expected: 5 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/__tests__/ClassifyButton.test.tsx
git commit -m "test(ui): ClassifyButton のプログレスバー・操作テスト追加"
```

---

## Task 13: Frontend — projectStore テスト

**Files:**
- Create: `src/__tests__/stores/projectStore.test.ts`

- [ ] **Step 1: Write the test file**

```ts
import { describe, it, expect, vi, beforeEach } from "vitest";
import { useProjectStore } from "../../stores/projectStore";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

describe("projectStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useProjectStore.setState({
      projects: [],
      selectedProjectId: null,
      loading: false,
      error: null,
    });
  });

  describe("fetchProjects", () => {
    it("sets projects on success", async () => {
      const projects = [
        { id: "p1", account_id: "acc1", name: "Project A", description: null, color: null, is_archived: false, created_at: "", updated_at: "" },
      ];
      mockInvoke.mockResolvedValue(projects);

      await useProjectStore.getState().fetchProjects("acc1");

      expect(mockInvoke).toHaveBeenCalledWith("get_projects", { accountId: "acc1" });
      expect(useProjectStore.getState().projects).toEqual(projects);
      expect(useProjectStore.getState().loading).toBe(false);
    });

    it("sets error on failure", async () => {
      mockInvoke.mockRejectedValue("DB error");

      await useProjectStore.getState().fetchProjects("acc1");

      expect(useProjectStore.getState().error).toBe("DB error");
      expect(useProjectStore.getState().loading).toBe(false);
    });
  });

  describe("selectProject", () => {
    it("sets selectedProjectId", () => {
      useProjectStore.getState().selectProject("p1");
      expect(useProjectStore.getState().selectedProjectId).toBe("p1");
    });

    it("clears selectedProjectId with null", () => {
      useProjectStore.getState().selectProject("p1");
      useProjectStore.getState().selectProject(null);
      expect(useProjectStore.getState().selectedProjectId).toBeNull();
    });
  });

  describe("deleteProject", () => {
    it("removes project from list and clears selection if selected", async () => {
      useProjectStore.setState({
        projects: [
          { id: "p1", account_id: "acc1", name: "A", description: null, color: null, is_archived: false, created_at: "", updated_at: "" },
          { id: "p2", account_id: "acc1", name: "B", description: null, color: null, is_archived: false, created_at: "", updated_at: "" },
        ],
        selectedProjectId: "p1",
      });
      mockInvoke.mockResolvedValue(undefined);

      await useProjectStore.getState().deleteProject("p1");

      expect(useProjectStore.getState().projects).toHaveLength(1);
      expect(useProjectStore.getState().projects[0].id).toBe("p2");
      expect(useProjectStore.getState().selectedProjectId).toBeNull();
    });

    it("keeps selection when deleting a different project", async () => {
      useProjectStore.setState({
        projects: [
          { id: "p1", account_id: "acc1", name: "A", description: null, color: null, is_archived: false, created_at: "", updated_at: "" },
          { id: "p2", account_id: "acc1", name: "B", description: null, color: null, is_archived: false, created_at: "", updated_at: "" },
        ],
        selectedProjectId: "p1",
      });
      mockInvoke.mockResolvedValue(undefined);

      await useProjectStore.getState().deleteProject("p2");

      expect(useProjectStore.getState().selectedProjectId).toBe("p1");
    });
  });

  describe("archiveProject", () => {
    it("removes project from list", async () => {
      useProjectStore.setState({
        projects: [
          { id: "p1", account_id: "acc1", name: "A", description: null, color: null, is_archived: false, created_at: "", updated_at: "" },
        ],
        selectedProjectId: "p1",
      });
      mockInvoke.mockResolvedValue(undefined);

      await useProjectStore.getState().archiveProject("p1");

      expect(useProjectStore.getState().projects).toHaveLength(0);
      expect(useProjectStore.getState().selectedProjectId).toBeNull();
    });
  });
});
```

- [ ] **Step 2: Run test to verify it passes**

Run: `pnpm test -- --run src/__tests__/stores/projectStore.test.ts`
Expected: 7 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/__tests__/stores/projectStore.test.ts
git commit -m "test(stores): projectStore の状態管理テスト追加"
```

---

## Task 14: Frontend — mailStore テスト

**Files:**
- Create: `src/__tests__/stores/mailStore.test.ts`

- [ ] **Step 1: Write the test file**

```ts
import { describe, it, expect, vi, beforeEach } from "vitest";
import { useMailStore } from "../../stores/mailStore";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

describe("mailStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMailStore.setState({
      threads: [],
      selectedThread: null,
      selectedMail: null,
      syncing: false,
      error: null,
    });
  });

  describe("fetchThreads", () => {
    it("sets threads on success", async () => {
      const threads = [
        { thread_id: "t1", subject: "Thread A", last_date: "2026-04-13", mail_count: 2, from_addrs: ["a@b.com"], mails: [] },
      ];
      mockInvoke.mockResolvedValue(threads);

      await useMailStore.getState().fetchThreads("acc1", "INBOX");

      expect(mockInvoke).toHaveBeenCalledWith("get_threads", { accountId: "acc1", folder: "INBOX" });
      expect(useMailStore.getState().threads).toEqual(threads);
    });

    it("sets error on failure", async () => {
      mockInvoke.mockRejectedValue("fetch error");

      await useMailStore.getState().fetchThreads("acc1", "INBOX");

      expect(useMailStore.getState().error).toBe("fetch error");
    });
  });

  describe("syncAccount", () => {
    it("sets syncing state and returns count", async () => {
      mockInvoke.mockResolvedValue(5);

      const count = await useMailStore.getState().syncAccount("acc1");

      expect(count).toBe(5);
      expect(useMailStore.getState().syncing).toBe(false);
    });

    it("returns 0 and sets error on failure", async () => {
      mockInvoke.mockRejectedValue("sync error");

      const count = await useMailStore.getState().syncAccount("acc1");

      expect(count).toBe(0);
      expect(useMailStore.getState().error).toBe("sync error");
      expect(useMailStore.getState().syncing).toBe(false);
    });
  });

  describe("selectThread", () => {
    it("sets selectedThread and clears selectedMail", () => {
      const thread = { thread_id: "t1", subject: "A", last_date: "", mail_count: 1, from_addrs: [], mails: [] };
      useMailStore.setState({ selectedMail: { id: "m1" } as never });

      useMailStore.getState().selectThread(thread);

      expect(useMailStore.getState().selectedThread).toEqual(thread);
      expect(useMailStore.getState().selectedMail).toBeNull();
    });

    it("clears selectedThread with null", () => {
      useMailStore.getState().selectThread(null);
      expect(useMailStore.getState().selectedThread).toBeNull();
    });
  });

  describe("selectMail", () => {
    it("sets selectedMail", () => {
      const mail = { id: "m1", subject: "Test" } as never;
      useMailStore.getState().selectMail(mail);
      expect(useMailStore.getState().selectedMail).toEqual(mail);
    });
  });
});
```

- [ ] **Step 2: Run test to verify it passes**

Run: `pnpm test -- --run src/__tests__/stores/mailStore.test.ts`
Expected: 7 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/__tests__/stores/mailStore.test.ts
git commit -m "test(stores): mailStore の状態管理テスト追加"
```

---

## Task 15: Frontend — classifyStore テスト

**Files:**
- Create: `src/__tests__/stores/classifyStore.test.ts`

- [ ] **Step 1: Write the test file**

```ts
import { describe, it, expect, vi, beforeEach } from "vitest";
import { useClassifyStore } from "../../stores/classifyStore";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

describe("classifyStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useClassifyStore.setState({
      classifying: false,
      classifyingAccountId: null,
      progress: null,
      results: [],
      summary: null,
      unclassifiedMails: [],
      error: null,
    });
  });

  describe("fetchUnclassified", () => {
    it("sets unclassifiedMails on success", async () => {
      const mails = [{ id: "m1", subject: "Test" }];
      mockInvoke.mockResolvedValue(mails);

      await useClassifyStore.getState().fetchUnclassified("acc1");

      expect(mockInvoke).toHaveBeenCalledWith("get_unclassified_mails", { accountId: "acc1" });
      expect(useClassifyStore.getState().unclassifiedMails).toEqual(mails);
    });

    it("sets error on failure", async () => {
      mockInvoke.mockRejectedValue("fetch error");

      await useClassifyStore.getState().fetchUnclassified("acc1");

      expect(useClassifyStore.getState().error).toBe("fetch error");
    });
  });

  describe("classifyMail", () => {
    it("appends result on success", async () => {
      const result = { mail_id: "m1", action: "assign", confidence: 0.9, reason: "test" };
      mockInvoke.mockResolvedValue(result);

      await useClassifyStore.getState().classifyMail("m1");

      expect(useClassifyStore.getState().results).toHaveLength(1);
      expect(useClassifyStore.getState().results[0]).toEqual(result);
      expect(useClassifyStore.getState().classifying).toBe(false);
    });

    it("sets error on failure", async () => {
      mockInvoke.mockRejectedValue("classify error");

      await useClassifyStore.getState().classifyMail("m1");

      expect(useClassifyStore.getState().error).toBe("classify error");
      expect(useClassifyStore.getState().classifying).toBe(false);
    });
  });

  describe("approveClassification", () => {
    it("removes mail from unclassified and results", async () => {
      useClassifyStore.setState({
        unclassifiedMails: [
          { id: "m1" } as never,
          { id: "m2" } as never,
        ],
        results: [
          { mail_id: "m1", action: "assign", confidence: 0.9, reason: "test" },
          { mail_id: "m2", action: "assign", confidence: 0.8, reason: "test" },
        ],
      });
      mockInvoke.mockResolvedValue(undefined);

      await useClassifyStore.getState().approveClassification("m1", "proj1");

      expect(useClassifyStore.getState().unclassifiedMails).toHaveLength(1);
      expect(useClassifyStore.getState().unclassifiedMails[0].id).toBe("m2");
      expect(useClassifyStore.getState().results).toHaveLength(1);
      expect(useClassifyStore.getState().results[0].mail_id).toBe("m2");
    });
  });

  describe("rejectClassification", () => {
    it("removes result but keeps mail in unclassified", async () => {
      useClassifyStore.setState({
        unclassifiedMails: [{ id: "m1" } as never],
        results: [{ mail_id: "m1", action: "assign", confidence: 0.5, reason: "test" }],
      });
      mockInvoke.mockResolvedValue(undefined);

      await useClassifyStore.getState().rejectClassification("m1");

      expect(useClassifyStore.getState().results).toHaveLength(0);
      // Mail stays in unclassified after rejection
      expect(useClassifyStore.getState().unclassifiedMails).toHaveLength(1);
    });
  });

  describe("classifyAll", () => {
    it("sets classifying state with accountId", async () => {
      mockInvoke.mockResolvedValue(undefined);

      // Don't await — classifyAll sets state then invokes
      const promise = useClassifyStore.getState().classifyAll("acc1");

      // classifying should be true immediately
      expect(useClassifyStore.getState().classifyingAccountId).toBe("acc1");

      await promise;
    });

    it("clears state on error", async () => {
      mockInvoke.mockRejectedValue("ollama down");

      await useClassifyStore.getState().classifyAll("acc1");

      expect(useClassifyStore.getState().error).toBe("ollama down");
      expect(useClassifyStore.getState().classifying).toBe(false);
      expect(useClassifyStore.getState().classifyingAccountId).toBeNull();
    });
  });
});
```

- [ ] **Step 2: Run test to verify it passes**

Run: `pnpm test -- --run src/__tests__/stores/classifyStore.test.ts`
Expected: 8 tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/__tests__/stores/classifyStore.test.ts
git commit -m "test(stores): classifyStore の状態管理テスト追加"
```

---

## Summary

| Task | Module | New Tests |
|------|--------|-----------|
| 1 | Rust: models/classifier | 4 |
| 2 | Rust: models/account | 8 |
| 3 | Rust: classifier/prompt | 4 |
| 4 | Rust: classifier/ollama | 9 |
| 5 | Rust: db/mails | 10 |
| 6 | Rust: mail_sync/mime_parser | 8 |
| 7 | Frontend: ClassifyResultBadge | 6 |
| 8 | Frontend: NewProjectProposal | 6 |
| 9 | Frontend: ThreadItem | 6 |
| 10 | Frontend: MailHeader | 6 |
| 11 | Frontend: ProjectForm | 5 |
| 12 | Frontend: ClassifyButton | 5 |
| 13 | Frontend: projectStore | 7 |
| 14 | Frontend: mailStore | 7 |
| 15 | Frontend: classifyStore | 8 |
| **Total** | | **104** |

Before: 79 Rust tests + 2 frontend test files
After: 122 Rust tests + 11 frontend test files (104 new tests total)
