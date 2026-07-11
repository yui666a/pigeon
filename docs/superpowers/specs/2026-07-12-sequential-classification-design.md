# 逐次分類（新規プロジェクト提案の1件ずつ承認）設計書

- 作成日: 2026-07-12
- ステータス: 承認済み（実装前）
- 関連: `2026-04-13-phase2-ai-classification-design.md`, `2026-07-11-classification-accuracy-improvement-design.md`

## 1. 目的

未分類メールの一括分類で、新規プロジェクト提案（`create`）が承認を待たずに次々表示され、同種のメールが同じ新規プロジェクトを重複提案する問題を解消する。

要望:
1. 新規プロジェクト提案は **1件ずつ** 出す（承認/却下するまで次を出さない）。
2. 「案件を作成」で承認したプロジェクトを **即座に左のプロジェクト一覧へ反映** する。
3. 以降のメール分類は、その **新プロジェクトも候補に含めて** 分類する。

## 2. 現状と原因

分類は2経路ある:
- `classify_mail(mail_id)`（単一メール分類。`build_project_summaries` を毎回読み、`create` は `PendingClassifications` に入れる）
- `classify_unassigned(account_id)`（**一括ループ**。未分類メールを最後まで回す）

原因は `classify_unassigned`（`src-tauri/src/commands/classify_commands.rs`）にある:

- `project_summaries` を **ループ前に1回だけ** 構築する（コード内コメントが「新規は承認時のみ挿入されるので再読込不要」と明言 — これが誤り）。
- `create` 提案は `PendingClassifications` に溜めるだけで、**承認を待たずに次のメールへ進む**。

結果、承認前の新規プロジェクトが後続メールの候補に入らず、同種メールが同じ新規プロジェクトを重複提案する。フロント（`UnclassifiedList`）は溜まった `create` 結果を `createResults.map(...)` で**複数カード縦積み**表示する。

## 3. 解決方針: フロント主導の逐次分類

バックエンドの一括ループを廃止し、**フロントが未分類メールを1件ずつ `classify_mail` で分類**する制御に変える。`classify_mail` は内部で `build_project_summaries` を毎回読むため、承認直後の新プロジェクトが自動的に次の分類候補へ入る（現状の「1回だけ構築」問題が原理的に消える）。

```
フロントのループ（classifyStore）:
  mails = get_unclassified_mails(accountId)
  total = mails.length; current = 0
  classifyNext()

classifyNext():
  中断フラグ or 残りなし → 完了
  mail = mails[current]
  res = classify_mail(mail.id); current++
  match res.action:
    "assign" / "unclassified" → classifyNext()      // 自動で次へ
    "create"                  → pendingProposal = res; return  // 停止（承認/却下待ち）

approveNewProject(mailId, name, desc):
  project = approve_new_project(...)   // 作成済み Project を返す
  projectStore に project を追加        // ★ 左一覧へ即反映（要望2）
  pendingProposal = null
  classifyNext()                       // ★ 次へ（新プロジェクト込みで継続, 要望3）

rejectClassification(mailId):
  reject_classification(mailId)        // 未分類のまま
  pendingProposal = null
  classifyNext()                       // 次へ

cancel():
  中断フラグを立てる（実行中の classifyNext 完了後に停止）
```

`create` 提案でのみ停止する（要望1）。`assign`（既存案件への自動割当）と `unclassified` は自動で次へ進む。却下したメールは未分類のまま残す。

## 4. バックエンドの変更（最小）

### 4.1 削除

一括ループとその周辺は逐次制御に置き換わるため削除する（agent.md「その場しのぎで残さない」）:

- `commands/classify_commands.rs`:
  - `classify_unassigned` 関数
  - `cancel_classification` 関数
  - `ClassifyCancelFlag` 構造体
  - `classify-progress` / `classify-complete` イベントの emit
- `lib.rs`:
  - `invoke_handler` から `classify_unassigned` / `cancel_classification` の登録を削除
  - `ClassifyCancelFlag` の `manage` 登録を削除
- 関連する `#[cfg(test)]` テスト（`ClassifyCancelFlag` 依存分）を削除

### 4.2 維持・再利用

- `classify_mail(mail_id)` — 逐次ループの単位として再利用（変更不要）。
- `get_unclassified_mails(account_id)` — フロントが対象一覧取得に使う（既存）。
- `approve_new_project(...)` — 作成した `Project` を返す（既存）。フロントがこれを一覧へ反映。
- `reject_classification(mail_id)` / `PendingClassifications` — 維持。

## 5. フロントの変更（主体）

### 5.1 `src/stores/classifyStore.ts`

- state を逐次制御に作り替える:
  - 追加: `pendingProposal: ClassifyResponse | null`（現在停止中の1件の提案。null なら停止していない）
  - 追加（内部）: 対象メール配列とインデックス、中断フラグ（クロージャ or state）
  - 変更: `progress: { current, total }` はフロントで数える（イベント購読をやめる）
- メソッド:
  - `classifyAll(accountId)` → `get_unclassified_mails` で一覧取得し逐次ループ開始（`invoke("classify_unassigned")` をやめる）
  - 内部 `classifyNext()` → 上記フロー
  - `approveNewProject(...)` → `approve_new_project` の戻り `Project` を `projectStore` に追加し `classifyNext()`
  - `rejectClassification(mailId)` → `reject_classification` 後 `classifyNext()`
  - `cancelClassification()` → 中断フラグを立てる（`invoke("cancel_classification")` をやめる）
  - `initClassifyListeners` → `classify-progress`/`classify-complete` 購読を削除（イベント自体が無くなる）

### 5.2 `src/stores/projectStore.ts`

- 既に作成済みの `Project` を一覧へ追加するヘルパを追加:
  ```ts
  addProject: (project: Project) => void   // set(projects: [...projects, project])
  ```
  （`fetchProjects` の再取得ではなく、承認で返ったプロジェクトを差し込むことで即時・低コストに反映）

### 5.3 `src/components/thread-list/UnclassifiedList.tsx`

- `createResults.map(...)` による複数カード表示をやめ、`pendingProposal` があるときだけ **1件** の `NewProjectProposal` を表示する。

### 5.4 `src/components/thread-list/ClassifyButton.tsx`

- `classifyAll` / `cancelClassification` の呼び出しは維持（store 側の実装が変わるのみ）。進捗・キャンセル表示は `classifying` / `progress` を引き続き参照。

## 6. テスト方針（TDD: Red → Green → Refactor）

### フロント（Vitest + RTL、主体）

- `classifyStore`（`classify_mail` / `get_unclassified_mails` / `approve_new_project` / `reject_classification` を invoke モック）:
  - `assign` 結果 → 自動で次の `classify_mail` を呼ぶ（連続進行）。
  - `create` 結果 → 停止し `pendingProposal` に1件セット、次の `classify_mail` を呼ばない。
  - `approveNewProject` → `projectStore` に project 追加、`pendingProposal` クリア、次へ進む。
  - `rejectClassification` → `pendingProposal` クリア、次へ進む。
  - `cancelClassification` → 以降 `classify_mail` を呼ばない。
  - `progress.current` が処理ごとに増える。
- `UnclassifiedList`: `pendingProposal` があるとき提案カードが **1件だけ** 描画される。無いとき描画されない。
- `projectStore`: `addProject` が既存配列に追加する。

### Rust

- 削除に伴い、`ClassifyCancelFlag`/`classify_unassigned` 依存のテストを除去。
- `classify_mail` / `approve_new_project` / `reject_classification` の既存テストは維持（挙動不変）。

## 7. 影響ファイル一覧

**Rust**
- `src-tauri/src/commands/classify_commands.rs` — `classify_unassigned`/`cancel_classification`/`ClassifyCancelFlag` と関連テスト削除
- `src-tauri/src/lib.rs` — command 登録と `ClassifyCancelFlag` の manage 削除

**フロント**
- `src/stores/classifyStore.ts` — 逐次制御へ全面変更
- `src/stores/projectStore.ts` — `addProject` 追加
- `src/components/thread-list/UnclassifiedList.tsx` — 1件表示へ
- `src/components/thread-list/ClassifyButton.tsx` — 参照確認（大きな変更なし）
- 関連テスト（`classifyStore.test.ts`, `NewProjectProposal.test.tsx` 等）を新挙動に更新

## 8. スコープ外（YAGNI）

- 分類の並列化・高速化（逐次で1件ずつ）。
- `create` 以外（assign/unclassified）での停止。
- 却下メールの再提案抑制（1件ずつ判断するため許容）。
- 確信度閾値（`CONFIDENCE_AUTO_ASSIGN`/`CONFIDENCE_UNCERTAIN`）の変更。
