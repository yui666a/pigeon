# Pre-Phase 4 リファクタリングレビュー

**作成日**: 2026-04-14
**更新日**: 2026-04-15
**目的**: Phase 4（検索・送信）に進む前に、Phase 1〜3 で蓄積された技術的負債を洗い出し、対処方針を定める
**レビュー対象**: フロントエンド（React/TypeScript）全ファイル、バックエンド（Rust）全ファイル

---

## 概要

| 重大度 | 件数 | 主なテーマ |
|--------|------|-----------|
| HIGH | 3 | セキュリティ、コード重複、規約違反 |
| MEDIUM | 9 | 設計改善、パフォーマンス、トランザクション管理 |
| LOW | 6 | コード品質、テスト補充、型安全性 |

> **注**: 初版で M-1 として記載していた「ThreadList のレース条件」は、実コードで `.then()` によるチェーンが既に実装済みであったため削除し、Codex レビューで新たに発見された問題を追加した。

---

## HIGH — 即時対応が必要

### H-1. XSS 脆弱性（MailView.tsx）

**ファイル**: `src/components/mail-view/MailView.tsx`
**行**: 38

**問題**:
`dangerouslySetInnerHTML` で `body_html` をサニタイズせずに描画している。
`dangerouslySetInnerHTML` は `<script>` タグを直接実行しないが、`onerror`, `onload` 等のインラインイベント属性や `javascript:` URL スキーム、`<iframe>` の埋め込み等を通じて任意のコードが実行される。メールクライアントにおいて、受信メールの HTML をサニタイズなしで描画するのは重大なリスク。

```tsx
// 現状（危険）
<div dangerouslySetInnerHTML={{ __html: mail.body_html }} />
```

**対策**:
- DOMPurify を導入し、描画前にサニタイズする
- イベント属性（`on*`）、危険な URL スキーム（`javascript:`）、`<iframe>` 等を除去する

```tsx
// 改善案
import DOMPurify from "dompurify";

<div dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(mail.body_html) }} />
```

---

### H-2. ドラッグロジックの重複（ThreadItem / UnclassifiedList）

**ファイル**:
- `src/components/thread-list/ThreadItem.tsx` — ドラッグハンドラ実装
- `src/components/thread-list/UnclassifiedList.tsx` — 内部の `MailDragItem` に同一ロジック

**問題**:
mousedown → mousemove → mouseup のドラッグ処理（5px スレッショルド判定、dragStore 操作、グローバルイベントリスナーの登録/解除）が 2 箇所に重複している。
変更時に片方だけ修正して不整合が生じるリスクがある。

また、`MailDragItem` が `UnclassifiedList.tsx` 内にインラインで定義されており、agent.md の「1ファイル1コンポーネント」ルールにも違反している。

**対策**:
1. `src/hooks/useDrag.ts` にカスタムフックとして抽出する
2. `MailDragItem` を `src/components/thread-list/MailDragItem.tsx` に分離する
3. `ThreadItem` と `MailDragItem` の両方がフックを使うようにする

```ts
// hooks/useDrag.ts（イメージ）
export function useMailDrag(mailIds: string[], label: string) {
  const { startDrag, updatePosition, endDrag } = useDragStore();
  // mousedown/mousemove/mouseup のロジックを集約
  // threshold: 5px
  return { onMouseDown };
}
```

---

### H-3. store の直接 setState（ThreadList.tsx）

**ファイル**: `src/components/thread-list/ThreadList.tsx`
**行**: 31, 34

**問題**:
`useMailStore.setState()` を直接呼び出してストアの内部状態を書き換えている箇所が **2 箇所** ある。
ストアのカプセル化が破壊され、状態遷移の追跡が困難になる。

```tsx
// 行 31: プロジェクトビューでの合成スレッド設定
useMailStore.setState({ threads: projectThreads });

// 行 34: エラー時のクリア
useMailStore.setState({ threads: [] });
```

**対策**:
mailStore に `setThreads(threads: Thread[])` アクションを追加し、コンポーネントからはそれを呼ぶ。
あるいは、バックエンドの `get_mails_by_project` をスレッド返却対応にして、クライアント側の合成自体を不要にする（→ M-2 参照）。

---

### ~~H-4.~~ → M-9 に移動

初版で H-4 として記載していた Rust `expect()` 使用は、影響範囲を再評価し MEDIUM に格下げした（後述 M-9）。

---

## MEDIUM — Phase 4 開始前に対処推奨

### ~~M-1. ThreadList のレース条件~~ — 対処済み・削除

**理由**: 実コード（`ThreadList.tsx:38`）では `syncAccount(...).then(() => fetchThreads(...))` で順次実行されており、レース条件は存在しない。初版の指摘は誤りであった。

ただし、ビュー切替やアカウント切替時に前回の非同期処理が完了する前に次の処理が走る（stale update）リスクは残る。Phase 4 以降で cleanup 関数による中断処理を検討すること。

---

### M-2. クライアント側でのスレッド合成

**ファイル**: `src/components/thread-list/ThreadList.tsx` — 行 23-30

**問題**:
プロジェクトビューで `Mail[]` → `Thread[]` の変換をフロントエンドで手動実行している。
バックエンドには `build_threads` 関数（Union-Find + 件名フォールバック）が既にあるが、`get_mails_by_project` コマンドは `Vec<Mail>` を返す設計。

**対策**:
- バックエンド: `get_threads_by_project(project_id)` コマンドを新設し、内部で `build_threads` を呼ぶ
- フロントエンド: フロントでのスレッド合成ロジックを削除する

> **注**: 現在 `get_mails_by_project` は `classify_commands.rs:413` に定義されている。新コマンドの追加先は `mail_commands.rs` に統一するか、`classify_commands.rs` に残すか方針を決めること。データ取得系コマンドの配置ルールを明確にする機会でもある。

---

### M-3. DB ロック競合（classify_unassigned）

**ファイル**: `src-tauri/src/commands/classify_commands.rs`
**関数**: `classify_unassigned()`

**問題**:
`DbState` が `Mutex<Connection>` で全コマンドが排他ロック。
`classify_unassigned()` のループ内では、毎イテレーションで DB ロック取得 → `build_project_summaries()` 再呼出 → 分類結果保存を繰り返しており、非効率。

コメントには「projects may have been created」とあるが、ループ内で新規プロジェクトが作成されるのは `Create` アクションが pending に入る場合のみで、実際の insert はユーザーの approve 操作まで発生しない。よって毎回の再取得は不要。

**対策**:
1. ループ開始前にプロジェクトサマリをまとめて取得する。`Create` アクションでプロジェクトが実際に追加された場合のみ再取得する
2. 分類結果の保存はバッチで行う（例: 10件ごと）
3. 将来的には r2d2 や deadpool による接続プールの導入を検討

---

### M-4. CSP 未設定（tauri.conf.json）

**ファイル**: `src-tauri/tauri.conf.json`

**問題**:
`security.csp` が `null` でフルオープン。H-1 の XSS リスクを増大させている。

**対策**:
適切な CSP ポリシーを設定する。

```json
{
  "security": {
    "csp": "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:"
  }
}
```

---

### M-5. 未使用 useEffect（AccountForm.tsx）

**ファイル**: `src/components/sidebar/AccountForm.tsx`
**行**: 37-41

**問題**:
OAuth 完了を検知する effect が空のまま残されている。デッドコードで混乱を招く。

**対策**:
実装するか、コメントごと削除する。

---

### M-6. React.memo の欠如（リストアイテム）

**ファイル**:
- `src/components/thread-list/ThreadItem.tsx`
- `src/components/thread-list/UnclassifiedList.tsx` 内の `MailDragItem`

**問題**:
リスト内で描画されるアイテムコンポーネントがメモ化されていない。
メール件数が増加すると不要な再レンダリングが増える。

**対策**:
`React.memo` でラップし、props の浅い比較で不要な再レンダリングを防ぐ。

---

### M-7. viewMode が App ローカル状態

**ファイル**: `src/App.tsx`

**問題**:
`viewMode`（"threads" | "unclassified" | "project"）が `App.tsx` の `useState` で管理されており、他のコンポーネントからアクセスできない。
Phase 4 で検索ビューが追加される際にさらに問題になる。

**対策**:
- Zustand ストア（`uiStore` 等）に移動する
- 検索結果ビュー等の将来の画面モードも一元管理できるようにする

---

### M-8. 二重フェッチパターン

**ファイル**:
- `src/components/sidebar/ProjectTree.tsx` — 行 70, 93, 101, 114 の **4 箇所** で `fetchProjects()` を手動呼び出し
- `src/components/thread-list/UnclassifiedList.tsx` — 行 93 で `fetchProjects()` を `await` なしで呼んでいる

**問題**:
store のアクション実行後にコンポーネント側で手動リフレッシュしている。
呼び忘れやタイミングの不整合が生じやすい。

さらに、`projectStore.ts` 内の `updateProject`（行 70-80）は楽観更新で既にローカル state を書き換えているにもかかわらず、ProjectTree 側でさらに `fetchProjects()` を呼んで全件再取得している。`archiveProject`（行 86-98）・`deleteProject`（行 101-114）も同様にストア内で `filter()` 済みなのに再フェッチしており、二重更新になっている。

UnclassifiedList の `await` なし呼び出しについては、`fetchProjects` 内部で `try/catch` + `error` state 設定をしているため、エラーが完全に握りつぶされるわけではない。ただし、`await` がないため分類完了後の画面更新順序が保証されない。

**対策**:
store が楽観更新しているアクション（`updateProject`, `archiveProject`, `deleteProject`）の呼び出し元からは `fetchProjects()` を削除する。store 側の楽観更新に一本化する。

```ts
// ProjectTree.tsx — submitRename の改善例
const submitRename = async () => {
  if (renamingProjectId && renameValue.trim()) {
    await updateProject(renamingProjectId, renameValue.trim());
    // fetchProjects() は不要 — store 内で楽観更新済み
  }
  setRenamingProjectId(null);
  setRenameValue("");
};
```

---

### M-9. Rust 規約違反: expect() 使用（ollama.rs）

**ファイル**: `src-tauri/src/classifier/ollama.rs`
**関数**: `OllamaClassifier::new()`

**問題**:
`reqwest::Client` の構築で `.expect()` を使用しており、agent.md の「テストコード以外で unwrap() / expect() を使用しない」ルールに違反している。

```rust
// 現状（違反）
let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(30))
    .build()
    .expect("Failed to build HTTP client");
```

**対策**:
`new()` を `Result<Self, AppError>` を返すように変更する。呼び出し元で `?` で伝播させる。

> **注**: `reqwest::Client::builder().build()` が失敗するのは TLS バックエンド初期化失敗等の極めて稀なケースのみであり、実行時の影響は低い。ただし規約遵守の観点から修正する。

---

### M-10. ProjectTree での非 null アサーション乱用 → **LOW に格下げ**

> **格下げ理由**: コンポーネント先頭（行 51-53）に `if (!selectedAccountId) return null;` の早期リターンがあるため、ランタイムで `null` になるパスは現状存在しない。ただし型レベルでは保証されていないため、LOW として改善を推奨する。詳細は L-6 に移動。

---

### M-11. approve_new_project のトランザクション管理（新規追加）

**ファイル**: `src-tauri/src/commands/classify_commands.rs`
**関数**: `approve_new_project()`
**行**: 333, 352, 359

**問題**:
手動の `BEGIN` / `COMMIT` / `ROLLBACK` を `conn.execute()` で実行している。
rusqlite には `Transaction` API があり、Drop 時に自動ロールバックされるため、手動管理は不要でありエラーが紛れやすい。

```rust
// 現状（手動トランザクション）
conn.execute("BEGIN", []).map_err(|e| e.to_string())?;
// ...
conn.execute("COMMIT", []).map_err(|e| e.to_string())?;
// エラー時:
let _ = conn.execute("ROLLBACK", []);
```

**対策**:
`conn.transaction()` を使用し、クロージャ内で処理を行う。

```rust
// 改善案
let tx = conn.transaction().map_err(|e| e.to_string())?;
let project = projects::insert_project(&tx, &req).map_err(|e| e.to_string())?;
assignments::assign_mail(&tx, &mail_id, &project.id, "user", Some(1.0))
    .map_err(|e| e.to_string())?;
tx.commit().map_err(|e| e.to_string())?;
```

---

## LOW — 改善推奨（Phase 4 以降でも可）

### L-1. 日付フォーマットの不統一

**ファイル**:
- `src/components/thread-list/ThreadItem.tsx` — `${month}/${date}`（ロケール無視）
- `src/components/mail-view/MailHeader.tsx` — `ja-JP` ロケール使用

**対策**:
`src/utils/date.ts` にフォーマット関数を作成し統一する。

---

### L-2. ハードコードされた日本語 UI テキスト

**ファイル**: 複数コンポーネント

**問題**:
`"アカウント追加"`, `"案件を追加"`, `"未分類"` 等の文字列がコンポーネントに直接埋め込まれている。

**対策**:
個人利用のため低優先度。将来的に i18n を導入するなら `src/i18n/` に定数を集約する。

---

### L-3. commands 層のテスト不足（Rust）

**ファイル**: `src-tauri/src/commands/` 配下の 5 ファイル（合計約 900 行）

**問題**:
Tauri コマンドハンドラにテストがゼロ。他の層は合計 132 テストと充実しているが、コマンド層の結合テストがない。

**内訳（2026-04-15 時点）**:
| モジュール | テスト数 |
|-----------|---------|
| db/ | 58 |
| mail_sync/ | 37 |
| classifier/ | 25 |
| models/ | 12 |
| commands/ | **0** |
| **合計** | **132** |

**対策**:
- Tauri の `tauri::test` ユーティリティを使用して結合テストを追加する
- 特に `classify_unassigned` のエラーパスとキャンセル処理を重点的にテストする

---

### L-4. classifyStore の責務過多

**ファイル**: `src/stores/classifyStore.ts`（185 行）

**問題**:
分類実行・結果管理・未分類メール一覧・メール移動の 4 つの責務を 1 つのストアが担当している。

**対策**:
- `classifyStore`: 分類実行・結果管理に集中
- `unclassifiedStore` または `mailStore` に移動: 未分類メール一覧、moveMail

---

### L-5. IMAP セッションの cleanup

**ファイル**: `src-tauri/src/commands/mail_commands.rs`
**行**: 94

**問題**:
`let _ = session.logout().await` で logout エラーを握りつぶしている。

**対策**:
エラーをログ出力する（接続断の場合は warn レベル）。

```rust
// 改善案
if let Err(e) = session.logout().await {
    eprintln!("[warn] IMAP logout failed: {}", e);
}
```

---

### L-6. ProjectTree での非 null アサーション（M-10 から格下げ）

**ファイル**: `src/components/sidebar/ProjectTree.tsx`
**行**: 70, 93, 101, 114

**問題**:
`selectedAccountId!` の非 null アサーション（`!`）が複数箇所で繰り返し使用されている。
コンポーネント先頭（行 51-53）に `if (!selectedAccountId) return null;` があるためランタイムでは安全だが、TypeScript の型レベルでは保証されておらず、将来の変更で早期リターンが除去された場合にエラーとなる。

**対策**:
早期リターン直後にローカル変数に束縛して型を絞る。

```tsx
if (!selectedAccountId) return null;
const accountId = selectedAccountId; // string 型に絞られる
// 以降 accountId を使用
```

---

## リファクタリング対象ファイル一覧

Phase 4 に入る前に変更が必要なファイルを一覧にまとめる。

### フロントエンド

| ファイル | 関連 Issue | 変更内容 |
|----------|-----------|----------|
| `src/components/mail-view/MailView.tsx` | H-1 | DOMPurify によるサニタイズ追加 |
| `src/components/thread-list/ThreadItem.tsx` | H-2, M-6 | ドラッグをフックに置換、React.memo |
| `src/components/thread-list/UnclassifiedList.tsx` | H-2, M-6, M-8 | MailDragItem 分離、フック使用、await 追加 |
| `src/components/thread-list/ThreadList.tsx` | H-3, M-2 | setState 除去、スレッド合成をバックエンド移行 |
| `src/hooks/useDrag.ts` | H-2 | 新規作成: ドラッグカスタムフック |
| `src/components/thread-list/MailDragItem.tsx` | H-2 | 新規作成: 分離コンポーネント |
| `src/components/sidebar/AccountForm.tsx` | M-5 | 未使用 effect 削除 |
| `src/components/sidebar/ProjectTree.tsx` | M-8, L-6 | 冗長な fetchProjects 呼び出し削除、非 null アサーション改善 |
| `src/App.tsx` | M-7 | viewMode を store に移動 |
| `src/stores/projectStore.ts` | M-8 | アクション内でリフレッシュ完結 |
| `src/stores/classifyStore.ts` | M-8 | 同上 |
| `src/stores/mailStore.ts` | H-3 | setThreads アクション追加 |

### バックエンド

| ファイル | 関連 Issue | 変更内容 |
|----------|-----------|----------|
| `src-tauri/src/classifier/ollama.rs` | M-9 | expect() → Result に変更 |
| `src-tauri/src/commands/classify_commands.rs` | M-3, M-11 | DB ロック最適化、Transaction API 使用 |
| `src-tauri/src/commands/mail_commands.rs` | M-2, L-5 | get_threads_by_project 追加、logout エラーログ |
| `src-tauri/src/db/mails.rs` | M-2 | プロジェクト別スレッド取得関数追加 |
| `src-tauri/tauri.conf.json` | M-4 | CSP ポリシー設定 |

---

## 変更履歴

| 日付 | 変更内容 |
|------|----------|
| 2026-04-14 | 初版作成 |
| 2026-04-15 | Codex レビュー第1回を反映: M-1 削除（対処済み）、H-4 → M-9 に格下げ、H-1 の技術的説明を修正、H-3 に 2 箇所目を追記、L-5 のファイルパスを修正（`imap_client.rs` → `mail_commands.rs:94`）、M-8 の該当箇所を 4 箇所に修正、L-3 のテスト数を 132 に修正、M-10（非 null アサーション）・M-11（トランザクション管理）を新規追加、M-2 にコマンド配置方針の検討を追記、リファクタリング対象ファイル一覧を更新 |
| 2026-04-15 | Codex レビュー第2回を反映: 概要テーブルの MEDIUM 件数を 10→9 に修正（M-10 を L-6 に格下げ）、LOW を 5→6 に修正、H-2 コード例の `updateDrag` → `updatePosition` に修正、M-8 の問題説明を改善（楽観更新との二重更新を指摘、`await` なしの影響を順序制御の問題に訂正）、M-8 の改善コード例を現行シグネチャに合わせて修正、L-3 の説明文を「他の層は合計132テスト」に修正し内訳表に commands/0 を追加、M-10 を L-6 に格下げ（早期リターンで実行時は安全） |
