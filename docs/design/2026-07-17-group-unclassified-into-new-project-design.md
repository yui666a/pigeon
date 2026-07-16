# 未分類メールをグルーピングして新規案件を作成する機能 設計書

- 作成日: 2026-07-17
- ステータス: 承認済み（実装前）
- 関連: `docs/design/2026-07-13-bulk-actions-design.md`（一括操作の共通基盤）、`docs/design/2026-07-09-project-directory-context-design.md`（送信可否ポリシー）、`docs/design/2026-04-13-phase2-ai-classification-design.md`（分類・新規案件提案）

## 1. 背景と目的

未分類リストでは既に複数メールを選択して「案件へ移動（既存案件）」「アーカイブ」「削除」ができる。
しかし、**選択したメール群をまとめて新しい案件として切り出す**手段がない。ユーザーは
先に案件を作ってから移動する2ステップを強いられる。

本機能は、未分類メールを複数選択した状態から **ワンアクションで新規案件を作成し、
選択メールをその案件へ一括移動する** フローを提供する。案件名・説明は LLM が選択メール群から
提案し、ユーザーが編集して確定する。

## 2. スコープ

### やること
- 未分類リストの一括操作バー（`BulkActionBar`）に「＋ 新しい案件」ボタンを追加
- 押下で LLM が選択メール群から案件名・説明を提案
- 提案を初期値とした編集可能なフォームを表示（既存 `ProjectForm` ベース）
- 「作成」で案件を新規作成し、選択メールをその案件へ一括移動

### やらないこと（YAGNI）
- 既存案件への「マージ提案」（今回は新規作成のみ）
- 提案の複数候補提示（1候補のみ。編集可能なので十分）
- 案件フォルダ選択の必須化（任意のまま）

## 3. 全体フロー

```
[未分類リストで複数メール選択]
   │
   │「＋ 新しい案件」押下
   ▼
suggest_project_from_mails(accountId, mailIds)   ← 新規: LLMで名前/説明を提案
   │  （取得中はフォーム内でローディング表示）
   ▼
NewProjectFromSelectionForm を展開（提案を初期値・編集可能）
   │
   │「作成（N件を移動）」押下
   ▼
createProject(accountId, name, description, color, dir?)   ← 既存
   │
   ▼
bulkMoveMails(selectedMailIds, newProjectId)              ← 既存
   │
   ▼
clearSelection() + fetchUnclassified(accountId)           ← 既存
```

既存部品の合成が中心で、新規に増えるのは「複数メールから案件名を提案する LLM 呼び出し」と、
それを使う展開フォーム UI のみ。

## 4. コンポーネント設計

### 4.1 Rust: 提案 LLM 呼び出し

| 追加箇所 | 内容 |
|---|---|
| `classifier/service.rs` `suggest_project_name(...)` | 選択メールの (件名・送信者・本文冒頭) リストを受け取り、共通の案件名・説明を1つ提案する。`TextGenerator::generate_text` を1回呼ぶ。応答は JSON（`{ name, description }`）としてパースし、既存 `sanitize_proposed_text` で正規化（制御文字除去・文字数上限: 名前 `PROPOSED_NAME_MAX_CHARS`、説明も同様の上限）。 |
| `classifier/prompt.rs` | 提案用のプロンプト生成関数を追加。「以下の複数メールに共通する案件を1つ命名し、簡潔な説明を付けよ。JSON `{name, description}` で答えよ」＋ メール群の列挙。 |
| `commands/classify_commands.rs` `suggest_project_from_mails` | Tauri command。`accountId` と `mailIds: Vec<String>` を受け取り、各 mail_id を既存の `db::mails::get_mail_by_id` で取得（単一メール分類の command と同じ経路）して件名・送信者・本文冒頭を集める。`suggest_project_name` を呼び、`{ name, description }` を返す。戻り値は `Result<ProjectSuggestion, String>`。 |

**パース失敗時**: 名前・説明ともに空文字で返す（フォールバック）。フォームはユーザー入力可能なので
致命的ではない。パニックは起こさない（マルチバイト境界に注意、既存分類のフォールバックと同じ扱い）。

### 4.2 送信境界（セキュリティ）

`agent.md` セキュリティルールおよび `2026-07-09-project-directory-context-design.md` の送信可否ポリシーに従う。

- LLM へ送るのは **選択メールそれぞれの 件名・送信者・本文冒頭1000文字** のみ。
- 未分類メールは案件ディレクトリに紐づいていないため、**ディレクトリ由来コンテキストは一切送らない**。
- クラウド LLM 選択時は既存の警告フロー（`2026-07-10-llm-provider-selection-design.md`）がそのまま適用される。デフォルトは Ollama（ローカル）。

### 4.3 フロント: API / ストア

| 追加箇所 | 内容 |
|---|---|
| `src/api/classifyApi.ts` `suggestProjectFromMails(accountId, mailIds)` | `invoke("suggest_project_from_mails", ...)` のラッパ。戻り値型 `ProjectSuggestion { name: string; description: string }` を付ける。 |
| 型定義 | `ProjectSuggestion` を `src/types/` に追加。 |

案件作成・メール移動は既存ストアをそのまま使う。

- `useProjectStore.createProject(accountId, name, description, color)` — 案件作成（内部で `fetchProjects` 済み）
- `useMailStore.bulkMoveMails(mailIds, projectId)` — 一括移動（`BulkResult` を返す）

### 4.4 フロント: UI

| 追加/改修 | 内容 |
|---|---|
| 新規 `src/components/thread-list/NewProjectFromSelectionForm.tsx` | `ProjectForm` をベースにした展開フォーム。props: `accountId`, `selectedMailIds`, `onCreated`, `onCancel`。マウント時に `suggestProjectFromMails` を呼び、取得中はローディング表示、完了したら名前・説明を初期値化。ボタンラベルは「作成（N件を移動）」。名前空欄時は作成 disabled。 |
| 改修 `src/components/thread-list/BulkActionBar.tsx` | 「＋ 新しい案件」ボタンを追加し、押下で `onCreateProject` を発火。props に `onCreateProject: () => void` を追加。 |
| 改修 `src/components/thread-list/UnclassifiedList.tsx` | フォーム開閉 state を持ち、`BulkActionBar` の `onCreateProject` で開く。作成成功時に選択解除・未分類再取得。 |

**選択の固定**: フォームは開いた時点の選択 mailIds を保持する。フォーム表示中に選択が変わっても、
作成時は開いた時点の mailIds を使う（提案と作成の対象を一致させる）。

## 5. 作成 → 移動の合成ハンドラ

```ts
async function handleCreateAndMove(
  name: string,
  description: string | undefined,
  color: string | undefined,
  directoryPath: string | undefined,
  mailIds: string[],
) {
  const project = await createProject(accountId, name, description, color);
  // directoryPath があれば linkDirectory(project.id, directoryPath)
  const result = await bulkMoveMails(mailIds, project.id);
  clearSelection();
  fetchUnclassified(accountId);
  return result;
}
```

`ProjectForm` が持つ「案件フォルダ選択」を残す場合、作成後に既存の `linkDirectory` を呼んで紐づける。

## 6. エラーハンドリング

| 事象 | 挙動 |
|---|---|
| LLM 提案が失敗（ネットワーク/タイムアウト/パース不能） | フォームは空欄で開き、`useErrorStore` にエラー通知。ユーザーは手入力で続行可能。**提案失敗で作成フロー全体を止めない** |
| 案件作成は成功・メール移動が一部失敗 | `BulkResult` の失敗件数を toast 表示。案件は作成済みなので残す（手動で再移動可能） |
| 案件名が空のまま作成 | 既存 `ProjectForm` 同様に作成ボタン `disabled`（名前必須） |
| 提案取得中に選択が変わる | フォームは開いた時点の選択を固定して使う |

## 7. テスト（TDD）

### Rust
- `suggest_project_name`: StubLlm で
  - 正常な JSON 応答を name/description にパース
  - パース不能な応答で空フォールバック（パニックしない、マルチバイト安全）
  - 制御文字を含む提案のサニタイズ・文字数上限
  - 空メールリストの扱い
- `suggest_project_from_mails` command: メール取得 → 提案の経路（サービス層のスタブで）

### React
- `NewProjectFromSelectionForm`:
  - マウント時に提案ローディングが表示される
  - 提案が名前・説明の初期値に反映される
  - 名前空欄で作成ボタンが disabled
  - 作成クリックで `createProject` → `bulkMoveMails` の順に呼ばれる
  - 提案が失敗しても空フォームで表示され、手入力で作成できる
- `BulkActionBar`:
  - 「＋ 新しい案件」ボタンが `onCreateProject` を発火（既存テストに1ケース追加）

## 8. 段階的実装順（Stacked PR 候補）

1. Rust: `suggest_project_name` + `suggest_project_from_mails` command（テスト先行）
2. フロント: API/型 + `NewProjectFromSelectionForm`（テスト先行）
3. フロント: `BulkActionBar` ボタン追加 + `UnclassifiedList` 配線

依存関係: 3 は 1・2 に依存。1 と 2 は並行可能（型のみ先に固定）。
