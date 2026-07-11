# 分類精度の改善 設計書

- 作成日: 2026-07-11
- ステータス: 承認済み（実装前）
- 関連: `2026-04-13-phase2-ai-classification-design.md`, `2026-07-09-project-directory-context-design.md`, `2026-07-10-llm-provider-selection-design.md`

## 1. 目的

メール分類の精度を上げる。分類器に渡す**入力情報の質と量**を増やすことで、プロバイダ（Ollama / Claude）を問わず判定精度を改善する。プロンプト構築（`classifier::prompt`）はプロバイダ非依存なので、両方に効く。

背景: `llama3.1:8b` 使用時に分類精度が低かった。原因の一つは入力設計にある。現状 LLM に渡しているのは「件名・送信者・本文冒頭300文字」と、各案件の「name/description/context/直近件名5件」のみで、案件を判別する手がかりが薄い。

## 2. 現状（変更前）

分類プロンプトは `build_user_prompt`（`src-tauri/src/classifier/prompt.rs`）で組み立てられる。

- 分類対象メール: `subject` / `from_addr` / `date` / `body_preview`（**本文冒頭300文字**、`MailSummary::from_mail` の `.take(300)`）
- 既存プロジェクト: `id` / `name` / `description` / `recent_subjects`（**直近件名5件**、`get_recent_subjects(conn, id, 5)`）/ `context`（許可時のみ、最大800文字）
- 過去の訂正履歴: 最大20件

確信度の自動割当閾値は `CONFIDENCE_AUTO_ASSIGN = 0.7`（`models/classifier.rs`）。**本改善では閾値は変更しない。**

## 3. 変更方針

```
分類対象メール:  件名 + 送信者 + 日付 + 本文(300 → 1000文字)
プロジェクト材料: name / description / context
                + 直近件名(5 → 10件)
                + 【新】代表送信者（頻度上位・表示名付きアドレス, 最大5件）
```

### 3.1 本文プレビューの拡張（300 → 1000文字）

- `MailSummary::from_mail`（`src-tauri/src/models/classifier.rs`）の `.take(300)` を `.take(1000)` に変更。マルチバイトは `chars()` ベースなので境界破壊は起きない（既存挙動を踏襲）。
- 定数化する: マジックナンバーを避けるため `pub const BODY_PREVIEW_CHARS: usize = 1000;` を `models/classifier.rs` に定義し、`from_mail` から参照する。

### 3.2 プロジェクトの代表送信者を追加（最も効く部分）

案件に割り当て済みメールの送信者を集計し、頻出上位を LLM に渡す。「誰から来る案件か」は件名以上に強い手がかりになる。

- `ProjectSummary`（`models/classifier.rs`）に `pub top_senders: Vec<String>` を追加。
- 各要素は表示名付きアドレスそのまま（メールの `from_addr` の値。例: `丸井 <marui@example.com>`）。表示名・アドレス・ドメインはこの1フィールドに含まれるため、別フィールドには分割しない（YAGNI）。
- 新規クエリ `assignments::get_top_senders(conn, project_id, limit) -> Result<Vec<String>, AppError>` を追加。
  - 当該案件に割り当て済みのメールの `from_addr` を `GROUP BY` し、件数降順で上位 `limit` 件を返す。
  - 同数のときの順序は `from_addr` の昇順で安定させる（テスト可能にするため決定的に）。
- `build_project_summaries`（`db/projects.rs`）で `get_top_senders(conn, &p.id, 5)` を呼び、`top_senders` を埋める。

### 3.3 直近件名の件数を増やす（5 → 10件）

- `build_project_summaries` の `get_recent_subjects(conn, &p.id, 5)` を `10` に変更。

### 3.4 プロンプトへの反映

`build_user_prompt` のプロジェクト行に、`Recent subjects` と並べて `Frequent senders` 行を追加する。

```
- id: p1, name: 春公演, description: ...
  Recent subjects: 件名A; 件名B; ... (最大10件)
  Frequent senders: 丸井 <marui@example.com>; 田中 <tanaka@example.com>; ... (最大5件)
  Context: ...(許可時のみ)
```

- `top_senders` が空の案件は `Frequent senders` 行を出さない（`recent_subjects` と同じ条件付き出力パターン）。
- セパレータは `recent_subjects` と同じく `; `。

システムプロンプト（`SYSTEM_PROMPT`）の Rules に1文追記:

```
- The sender address is a strong signal; prefer a project whose frequent senders match the email's From.
```

## 4. セキュリティルールの更新（設計書ファースト）

本文送信文字数を 300 → 1000 に変更するため、関連ドキュメントの記述を更新する。

- `agent.md`（= `CLAUDE.md` が読む本体）のセキュリティルール:
  「本文冒頭300文字」→「本文冒頭1000文字」。
- `docs/superpowers/specs/2026-07-09-project-directory-context-design.md` に本文文字数の記述があれば整合させる。
- `docs/superpowers/specs/2026-07-10-llm-provider-selection-design.md` の警告バナー文言（「本文冒頭300文字」）→「本文冒頭1000文字」。
- UI: `LlmSettingsDialog.tsx` のクラウド警告バナー文言「本文冒頭300文字」→「本文冒頭1000文字」。

この変更はクラウド（Claude）にも適用される（前提合意: 精度をプライバシーの形式的制約より優先）。ローカル/クラウドで文字数は分けない。

## 5. テスト方針（TDD: Red → Green → Refactor）

### Rust

- `MailSummary::from_mail`:
  - 本文が1000文字ちょうどで切られること（301〜1000文字の本文が全て入り、1001文字目以降が落ちること）。
  - マルチバイト本文で `chars().count()` が1000になり境界が壊れないこと。
  - 既存の「300で切る」前提のテストを1000に更新する。
- `assignments::get_top_senders`:
  - 頻度降順で返ること（多い送信者が先頭）。
  - `limit` を超えないこと。
  - 同数時は `from_addr` 昇順で安定すること。
  - 割り当てメールが無い案件では空 `Vec` を返すこと。
- `build_project_summaries`:
  - `top_senders` が設定されること（割り当て済み送信者が反映される）。
  - `recent_subjects` が最大10件になること。
- `build_user_prompt`:
  - `top_senders` があるとき `Frequent senders:` 行が出て、各送信者を含むこと。
  - `top_senders` が空のとき `Frequent senders:` 行が出ないこと。
  - システムプロンプトに sender 活用の記述が含まれること（`SYSTEM_PROMPT` の定数テスト、または prompt 出力への非依存な確認）。

HTTP 実通信はテストしない（プロンプト整形とクエリのユニットが中心）。

### 影響する既存テスト

- `models/classifier.rs` の `test_from_mail_truncates_body_at_300_chars` 等、300 前提のテストを 1000 に更新。
- `classifier::prompt` の既存テストは `Frequent senders` 追加後も壊れないこと（`ProjectSummary` に `top_senders: vec![]` を足す必要がある。テストヘルパ `make_project` を更新）。

## 6. 影響ファイル一覧

**Rust**
- `src-tauri/src/models/classifier.rs` — `BODY_PREVIEW_CHARS` 定数、`from_mail` の1000化、`ProjectSummary.top_senders` 追加、テスト更新
- `src-tauri/src/db/assignments.rs` — `get_top_senders` 追加
- `src-tauri/src/db/projects.rs` — `build_project_summaries` で `top_senders` と件名10件
- `src-tauri/src/classifier/prompt.rs` — `Frequent senders` 行、`SYSTEM_PROMPT` 追記、テスト更新

**ドキュメント / UI**
- `agent.md` — セキュリティルールの文字数
- `docs/superpowers/specs/2026-07-10-llm-provider-selection-design.md` — 警告文言
- `docs/superpowers/specs/2026-07-09-project-directory-context-design.md` — 本文文字数の整合（該当があれば）
- `src/components/sidebar/LlmSettingsDialog.tsx` — 警告バナー文言

## 7. スコープ外（YAGNI）

- 確信度閾値（`CONFIDENCE_AUTO_ASSIGN` / `CONFIDENCE_UNCERTAIN`）の変更。
- 本文キーワード抽出、添付ファイル本文・スレッド履歴の投入。
- プロバイダ別の本文文字数切り替え（一律1000）。
- 送信者の表示名・アドレス・ドメインへの分割（1フィールドのまま）。
