# Pigeon リファクタリング調査レポート

**作成日**: 2026-07-13
**目的**: 機能実装が一段落した時点でのコードベース全体調査。バグ源・無駄なコメント・dead code・重複・パフォーマンス・メモリ効率・アーキテクチャ（Clean Architecture / DDD 拡張性）の観点から棚卸しし、リファクタリングの優先順位を定める。
**調査方法**: 観点別の並行コードレビュー（Rust db/commands層、Rust mail_sync/classifier層、フロントエンド、アーキテクチャ全体）+ 静的解析（cargo clippy / tsc / ts-prune）+ テストスイート実行 + 過去レビュー（`docs/2026-04-14-pre-phase4-refactoring-review.md`）の追跡。

---

## 0. ベースライン（調査時点の健全性）

| 項目 | 結果 |
|------|------|
| cargo test | **508 + 2 件 全パス** |
| vitest | **47ファイル 450件 全パス** |
| tsc --noEmit | エラーなし |
| cargo clippy | 警告 19件（needless_question_mark 11 / manual_contains 4 / new_without_default 2 / if_same_then_else 1 / needless as_bytes 1） |
| ts-prune | 未使用 export 1件（`ClassifySummary`） |

テストは全面グリーンで、リファクタリングを始めるには理想的な状態。clippy 警告は `cargo clippy --fix` でほぼ機械的に解消できる（18件自動修正可能）。

---

## 1. エグゼクティブサマリ

### 全体評価

コードベースは約3万行。**コメント品質・テスト網羅・純関数分離は全体的に高品質**で、特に `mail_sync` の純関数分離、`project_context` の責務分離とロック規律、FTS5/LIKE のインジェクション対策は模範的。危険な `unwrap()`/`expect()` は本体コードにほぼゼロ、機密情報のログ出力もなし。

一方で構造的な負債は明確に3系統ある:

1. **トランザクション境界の欠如**（db層の複数書き込みが非原子的 — 割り当て消失・マイグレーション起動不能のリスク）
2. **ユースケース層の不在**（commands層とZustandストアが業務ロジックを兼務 — commands層テスト0件の根本原因）
3. **重複コード**（classifier 4実装の classify 本体、plan_delete/plan_archive の3ファイル分散、row→struct マッピング等）

### 最優先で対処すべき High 指摘（バグリスク）

| # | 指摘 | 場所 |
|---|------|------|
| B-1 | `upsert_sent_mail` がトランザクション外 — 中断で重複行削除済み+uid未確定の中途半端な状態が残る | `db/mails.rs:288-328` |
| B-2 | `approve_classification` / `move_mail_to_project` の assignment更新+correction_log INSERT が非原子的 | `db/assignments.rs:28-62, 250` |
| B-3 | `auto_follow_threads` — 全メール本文込みロード + ループ内 N+1 クエリ + 非トランザクション一括書き込み。未分類一覧表示のたびに実行される | `db/assignments.rs:288-326` |
| B-4 | `delete_account` — 手動カスケードが非トランザクションで途中失敗すると不整合 | `db/accounts.rs:81-90` |
| B-5 | マイグレーションが非トランザクション + `ALTER TABLE ADD COLUMN` 非冪等 — 途中失敗で再実行時 "duplicate column" で**起動不能**の恐れ | `db/migrations.rs:151-236` |
| B-6 | `send_mail` の Sent uid 採番 TOCTOU（`get_max_uid+1` と insert の間に競合可能） | `commands/send_commands.rs:216-219` |
| B-7 | SMTP送信成功後のローカルDB挿入失敗が Err で返る — UIは「送信失敗」表示 → ユーザー再送で**二重送信** | `commands/send_commands.rs:174-220` |
| B-8 | `save_attachment` の `dest_path` 未検証 — IPC境界で任意パス書き込み可能 | `commands/attachment_commands.rs:161-171` |
| B-9 | メール削除時に添付キャッシュディレクトリ（`{data_dir}/Pigeon/attachments/{mail_id}/`）が孤児化しディスクリーク | `delete_mail` 呼び出し経路全般 |
| B-10 | `filter_map(\|r\| r.ok())` によるサイレントな行欠落（メール一覧・添付・ルール一覧が黙って欠ける） | `db/` 全域（attachments:59, cloud_rules:59, project_files:60, assignments 他多数） |
| B-11 | AccountForm — OAuth 成功後にフォームが閉じない（`oauthStatus` の `"idle"` が「未開始」と「成功」で二義的） | `src/components/sidebar/AccountForm.tsx:107-113` |
| B-12 | `useMailStore()` セレクタなし全体購読 ×3箇所 — 同期進捗の高頻度更新で一覧・本文が再描画され続ける | `ThreadList.tsx:22` / `MailView.tsx:9` / `UnclassifiedList.tsx:26` |
| B-13 | `useMailDrag` — ドロップ非成立時に `endDrag()` 未呼び出しの疑い（ゴースト残留） | `src/hooks/useMailDrag.ts:38-45` |
| B-14 | 共有テストヘルパが `PRAGMA foreign_keys = ON` を設定せず、**テストは FK 強制 OFF・本番は ON** という乖離。FK/CASCADE 依存のバグをテストが素通りさせる | `src-tauri/src/test_helpers.rs:13-23`（本番は `lib.rs:36`） |

### アーキテクチャの核心

**「あるべき姿」は既にコードベース内に2つ存在する**: `classifier` の trait port（`LlmClassifier`/`TextGenerator`）と `project_context::rescan_project`（`&dyn TextGenerator` 注入のユースケース関数）。同一コマンドファイル内で LLM 境界は本物の port なのに DB 境界は具象直呼びという非対称があり、リポジトリ trait 化は「新パラダイム導入」ではなく**既存パターンの水平展開**として進められる。

---

## 2. アーキテクチャ評価（Clean Architecture / DDD）

### 2.1 現状の層構造

| 層 | 現状 | 評価 |
|----|------|------|
| Entities（ドメイン） | ほぼ不在。`models/` は serde DTO | ✗ 貧血。不変条件はDBスキーマとSQL任せ |
| Use Cases | `commands/` とフロント stores が兼務 | ✗ 独立層なし |
| Interface Adapters | `db/`（具象直呼び）、`classifier`（trait ✓） | △ classifier のみ port 化 |
| Frameworks & Drivers | Tauri / rusqlite / async-imap / reqwest | ✓ 末端に寄っている |

**バックエンド**: `lib.rs:156-212` で56コマンドを登録。`DbState(Mutex<Connection>)` は単一コネクションの全体排他ロック。`classify_mail`（`classify_commands.rs:33-82`）が典型で、「メール取得 → サマリ構築 → 修正履歴取得 → 分類器構築 → LLM実行 → 確信度ゲート（`confidence >= CONFIDENCE_UNCERTAIN`）→ assign/pending 振り分け」というユースケース全体+業務ルールが Tauri ハンドラに直書きされている。`mail_commands.rs` も同期アルゴリズムの中核（`sync_account_inner`/`sync_folder_into`/`sync_sent_folder`、約280行）を commands 層に抱える。

**フロントエンド**: UI（components）と「状態+アプリケーション+インフラ融合」の stores の2層のみ。`invoke()` は全ストアに直書き（mailStore だけで約20箇所）、`src/api/` 相当の抽象層は存在しない。さらに `ThreadList.tsx:34,51` はコンポーネントから直接 `invoke("get_threads_by_project")` を呼ぶ層違反が残る。エラー処理 `catch(e){ set({error}); useErrorStore.getState().addError(String(e)) }` は全ストア全アクションにコピペされ、reauth 判定は `errorMsg.includes("Reauth required")` のマジックストリング照合（`mailStore.ts:215`）。

**テストの偏りが層の歪みを正確に映している**: commands層テストが書けない（`#[tauri::command]` と `State<'_, DbState>` に貼り付いているため）一方、業務ロジックが純関数に染み出した部分（`build_threads`、`auto_follow_threads`、`rescan_project`）はテストが充実している。

### 2.2 良い実例（横展開の軸）

**(a) classifier trait**（`classifier/mod.rs:15,24`）: `MailSummary`/`ProjectSummary`/`CorrectionEntry` → `ClassifyResult` というドメイン型のみで表現された本物の port。4アダプタ（ollama/claude/claude_vertex/gemini_vertex）が実装し `Box<dyn LlmClassifier>` で消費。`TextGenerator` supertrait の ISP 分離も適切。惜しい点: port が具象アダプタと同居、確信度閾値が DTO モジュール（`models/classifier.rs:3-7`）に散在、`build_classifier`（`factory.rs:40`）が `&Connection`+`&SecureStore` を直接取り設定読込とワイヤリングが融合。

**(b) `rescan_project`**（`project_context/mod.rs:27`）: `&dyn TextGenerator` を注入で受け、「ロックは短く、ファイルI/OとLLM呼び出しはロック外」という規律で8フェーズをオーケストレート。`MockGenerator`/`FailGenerator`/`SpyGenerator` 注入によるテストが秀逸で、**port 注入がテスト容易性に直結する好例**。

### 2.3 特筆すべき状態管理の歪み

- **`PendingClassifications`**（`classify_commands.rs:19`）: `Mutex<HashMap<mail_id, ClassifyResult>>` のインメモリのみ。「新規案件作成」提案はアプリ再起動で黙って失われる。さらに `approve_new_project`/`reject_classification` 以外の経路（`move_mail` で処理、放置）では**エントリが除去されずリーク**する。
- **旧M-3の「解消」は実はフロント移設**: バッチ分類ループはバックエンドから消え、`classifyStore.classifyNext()`（`classifyStore.ts:35-66`）として Zustand ストア内に移った。「未分類を全件、新規案件提案で一時停止、承認後に再開」という業務ワークフローが UI 層に居座り、N回の invoke 往復で駆動される。

### 2.4 DDD 拡張の設計案

**境界づけられたコンテキスト候補**:

| コンテキスト | 責務 | 現在の対応モジュール |
|-------------|------|---------------------|
| Mail Sync | IMAP/SMTP/MIME、同期watermark、フラグ双方向 | `mail_sync/`, `mail_commands`, `send_commands` |
| Classification | LLM分類、確信度ポリシー、スレッド追従、修正学習 | `classifier/`, `classify_commands`, `db/assignments` |
| Project Context | ファイルスキャン、ダイジェスト、クラウド送信ポリシー | `project_context/` |
| Search | FTS5、案件横断検索 | `db/search`, `search_commands` |

Account/Settings は共有カーネル。Classification⇔Mail Sync の腐敗防止層は `MailSummary::from_mail` として既に存在する。

**集約候補**: Project（+assignments+directories+contexts+cloud_rules）/ Thread（既読・スター・フォルダ移動のスレッド内一括更新は本来 Thread 集約の振る舞い。現在フロント mailStore のツリー操作ヘルパ群に分散）/ Account（+認証情報+IDLE監視）/ Mail（`uid_confirmed` の状態遷移）。

**ドメインサービス候補**: `ClassificationService`（確信度閾値+振り分け）、`ThreadBuilder`（`build_threads` 昇格）、`CredentialResolver`（OAuth リフレッシュ）、`CloudSendPolicy`（`cloud_policy::is_cloud_allowed` + 1000字制限を統合。現在 `project_context/cloud_policy.rs` と `models/classifier.rs` に分散）。

**リポジトリ導入の道筋**: 現 `db/*.rs` は `&Connection` 第一引数の「関数型リポジトリ」に近い。(1) trait 定義 + 既存関数へ委譲する `SqliteMailRepository` で非破壊に trait 化 → (2) 抽出したユースケース関数が `&dyn MailRepository` を受ける → (3) commands層テストがモックで書けるようになる。

### 2.5 目標ディレクトリ構成

```
src-tauri/src/                        src/
├── domain/      # エンティティ+VO+ドメインサービス   ├── api/        # 【新設】invoke集約・arg変換・エラー正規化
├── usecases/    # Tauri非依存・注入で動く            │   ├── client.ts / mailApi.ts / ...
├── ports/       # trait（repository / llm）          │   └── errors.ts  # reauth等を型で表現
├── adapters/                                         ├── stores/     # 状態のみに縮小
│   ├── sqlite/  # 現 db/                             ├── components/ # 現状維持
│   ├── llm/     # 現 classifier実装                   ├── hooks/ / utils/ / types/
│   └── imap_smtp/ # 現 mail_sync/
├── commands/    # 薄いTauriアダプタ
└── models/      # serde DTO（境界入出力専用に縮小）
```

---

## 3. Rust バックエンド詳細

### 3.1 db層

#### バグリスク（High）

- **`db/mails.rs:288-328` `upsert_sent_mail` のトランザクション欠如**: 内部で `get_mail_id_by_message_id` → `find_uid_occupant` → `merge_duplicate_sent_rows`（DELETE+UPDATE）/ `confirm_uid` / `insert_mail` を個別実行。`merge_duplicate_sent_rows` で DELETE 成功後に UPDATE が失敗すると中途半端な状態が残る。「案件割り当ての保持を最重要視」という設計意図と矛盾。→ `conn.unchecked_transaction()` で全分岐を包む（`update_flag_state` は既に採用済みのパターン）。
- **`db/assignments.rs:28-62` `approve_classification` / `:250` `move_mail_to_project`**: assignment 更新と `insert_correction` が非原子的。→ `merge_projects`（`projects.rs:144`、既に `conn.transaction()` を正しく使う唯一の手本）に揃える。
- **`db/assignments.rs:288-326` `auto_follow_threads`**: `get_all_mails_by_account` で本文込み全メールをロードし、メールごとに `get_assignment_info` を個別発行（O(N)〜O(2N) クエリの N+1）。`get_unclassified_threads` 経由で**未分類一覧を開くたびに実行される**ためパフォーマンス面でも最重要。→ assignments を1クエリで HashMap に先読み + 軽量カラムのみの SELECT + トランザクション化。
- **`db/accounts.rs:81-90` `delete_account`**: mails/projects の手動 DELETE + accounts DELETE が非トランザクション。→ トランザクション化。理想は mails に `ON DELETE CASCADE` を張り accounts 1回の DELETE でカスケードに任せる。
- **`test_helpers.rs:13-23` テストと本番の FK 強制乖離（B-14）**: 本番は `lib.rs:36` で `PRAGMA foreign_keys = ON` を設定するが、全テストが使う共有 `setup_db` は未設定（SQLite のデフォルトは OFF）。db 各モジュールの個別テストは手動で pragma を立てているのに共有ヘルパだけ抜けており一貫していない。→ `setup_db` 内 `run_migrations` 直後に pragma を追加し、各テストの手動設定は削除して一元化。
- **`db/migrations.rs:151-236`**: 各マイグレーションと `set_schema_version` が別ステートメントで、途中失敗時に「スキーマ一部適用済み + version 未更新」→ 再実行で `ALTER TABLE ADD COLUMN`（v7/v9/v10、非冪等）が "duplicate column" で失敗し**起動不能**。→ 「BEGIN → migrate_vN → set_schema_version → COMMIT」で1バージョン1トランザクションに。あわせて手書き `if version < N` 連鎖を `&[(i32, fn)]` テーブル+ループに変えると追記が1行で済む。

#### バグリスク（Medium）

- **`filter_map(|r| r.ok())` の全域的なサイレント握り潰し**: `attachments.rs:59` / `cloud_rules.rs:59` / `project_files.rs:60` / `assignments.rs`（get_unclassified_mails, get_mails_by_project 他多数）/ `projects.rs:66` / `drafts.rs:90` / `accounts.rs:76`。行デシリアライズ失敗が黙って欠落し、再現困難なデータ欠けバグの温床。→ `.collect::<rusqlite::Result<Vec<_>>>()?` に統一。
- **`db/mails.rs:131-153` `get_max_uid`/`get_min_uid` の `.unwrap_or(0)`**: DBロック・I/Oエラーまで 0 に丸め、同期 watermark が誤って 0 になると全件再取得や取りこぼしを誘発。`get_mail_id_by_message_id`（`.ok()`）等も同様。→ `?` 伝播 + 「行なし」は `OptionalExtension::optional()` で区別。
- **`db/settings.rs:5-12` `get_or_default`**: `QueryReturnedNoRows` 以外の障害まで default に丸める。→ NoRows のみフォールバック。
- **`db/accounts.rs:43-44 他` `try_from(...).unwrap_or(...)`**: 未知の `auth_type`/`provider` を黙って Plain/Other に丸める。OAuth アカウントが Plain 扱いになり原因不明の認証失敗になり得る。→ 少なくともログ、理想は Validation エラー化。
- **`db/projects.rs:80-81` `update_project`**: `Option::or` により description/color の明示的 NULL クリアが表現不可。仕様確認の上、`Option<Option<String>>` か意図をコメント明記。
- **`db/projects.rs:115-116` `build_project_summaries`**: サブクエリ失敗を `unwrap_or_default()` で空扱い → 分類プロンプト品質が静かに劣化。

#### アーキテクチャ・重複

- **db層にドメインロジックが混入**: `build_threads`/`normalize_subject`/`find_root`（`mails.rs:467-590`、DB アクセスを一切含まない純粋なスレッド判定アルゴリズム）、Sent マージの業務判断（`upsert_sent_mail`）、`auto_follow_threads`（複数集約をまたぐドメインサービス）、`build_project_summaries`（分類ドメインの関心事が projects の CRUD ファイルに混在）、`cloud_rules.rs:17,35` の末尾スラッシュ正規化（ドメインルール）。→ §2.4 の domain/usecases 層へ段階的に移動。
- **row→struct マッピングの不統一と重複**: `accounts.rs` では**同一10列マッピングが3箇所に完全コピペ**（32-48, 58-74, 100-116）。他ファイルも独立関数派（`row_to_attachment`/`map_row`）とインラインクロージャ派が混在。→ `Model::from_row(row) -> rusqlite::Result<Self>` を models 側に統一実装し、`query_map(params, Model::from_row)` の一行に。
- **`mails.rs:366-415` `mark_read`/`mark_unread`/`set_flagged` がほぼ同一の3コピー**（SELECT→NotFound→1カラムUPDATE→(folder,uid)返却）。→ 共通ヘルパに集約。
- **`MAIL_COLUMNS` と `MAIL_COLUMNS_PREFIXED` の二重メンテ**（`mails.rs:9-21`）: カラム追加時に4〜5箇所の同期更新が必要でズレると位置ずれバグ。→ 単一の `&[&str]` から生成。
- **「query_map で1行だけ取る」手動パターン**（`assignments.rs:190-197`, `accounts.rs:100-123`）→ `OptionalExtension::optional()` で置換。
- **`insert_project` / `insert_project_with_id` の INSERT 文重複**（`projects.rs:29-43`）→ 委譲（accounts.rs は既にこの形）。
- **`corrected_from` を書く UPDATE の重複**（`assignments.rs:52-57` と `projects.rs:167-171`）→ `reassign_with_correction(tx, ...)` に集約（訂正の記録方法というドメインロジックを1箇所に）。
- **`&Connection` / `&mut Connection` の不統一**: トランザクションを張る関数だけ `&mut` を要求し、呼び出し側が `&*guard`/`&mut *guard` を使い分ける複雑さが漏れる。→ 当面は呼び出し規約をドキュメント化、将来はリポジトリ構造体でシグネチャ統一。

#### パフォーマンス

- **`mails.rs:116-129` `get_all_mails_by_account`**: スレッド追従のたびに全フォルダ・全メールを body_text/body_html 込みでロード。→ スレッド判定用の軽量 struct + 専用 SELECT。
- **`search.rs:130` `search_like` の `mail.clone()`**: 本文含む Mail 全体を1行ごとに複製する完全に不要な clone。→ subject だけ clone して mail は move。
- `build_threads` の O(n²) 件名フォールバック（`mails.rs:498-516`）、`mails[i].clone()`（527行）→ HashMap 化 / 所有権を取る API に。
- インデックス: 未読集計用 `(account_id, folder, is_read, date)` 部分インデックス、`correction_log(corrected_at DESC)` を将来検討（現状データ量では Low）。

#### ファイル分割案

- **`db/mails.rs`（1452行、本体590行）** → ①永続化のみの `db/mails.rs` ②Sent マージを `db/sent_sync.rs` ③`build_threads` 系をドメインの `threading.rs` へ。
- **`db/assignments.rs`（1048行、本体約340行）** → CRUD（リポジトリ）と `approve_classification`/`move_mail_to_project`/`auto_follow_threads` 等のサービス関数を分離。
- **`db/migrations.rs`（1575行、本体467行）** → 本体は健全。テスト分離 + ディスパッチのテーブル化のみ。

### 3.2 commands層

#### バグリスク

- **（High）`send_commands.rs:216-219` Sent uid TOCTOU**: `get_max_uid+1` → `insert_mail` の間に同時送信・Sent同期が同一 uid を採番し得る（UNIQUE 制約で挿入失敗）。→ 採番と挿入を単一トランザクション or `INSERT ... SELECT COALESCE(MAX(uid),0)+1` で原子化。
- **（High）`send_commands.rs:174-220` 送信成功後のローカル反映失敗**: SMTP 送信成功後の DB 挿入失敗が Err で返り、UI は「送信失敗」→ 再送で二重送信。→ 送信成功後のローカル挿入はベストエフォート（警告ログのみ）に格下げ。次回 Sent 同期の message_id マージで復元される設計と整合的。
- **（High）`attachment_commands.rs:161-171` `save_attachment`**: フロントからの `dest_path` を無検証で `fs::copy`。IPC 境界は呼び出し元を信頼できない。→ 検証 or ダイアログハンドル経由に。
- **（High）添付キャッシュの孤児化**: `delete_mail`（単体・bulk とも）は DB 行と FTS を消すが `attachments/{mail_id}/` のディスクキャッシュを消さない。→ コマンド層で `remove_dir_all` をベストエフォート実行（db層に副作用を持たせない）。
- **（Medium）`classify_commands.rs:64-78` 低信頼度 Assign の分類漏れ**: `confidence < CONFIDENCE_UNCERTAIN` の Assign は `_ => {}` で何も永続化されず pending にも入らないが、フロントには提案が返る。承認時に割り当て不在で不整合の可能性。→ 仕様を確定し分岐を明示。
- **（Medium）`PendingClassifications` のリーク**: 除去経路が approve/reject のみ。`move_mail`/`approve_classification` 実行時にも該当 mail_id を除去すべき。＋再起動で消える揮発性の是非を設計判断（DBテーブル化の検討）。
- **（Medium）`auth_commands.rs:88-91`**: ループバック HTTP を固定8192バイト単一 read で処理（分割到着で OAuth コールバック取りこぼし）。→ リクエストラインが揃うまで read ループ。
- **（Medium）`auth_commands.rs:162`**: `now - pending.created_at` の減算アンダーフロー（時刻巻き戻りでパニック）。41,158行の `.expect("Time went backwards")` は規約違反でもある。→ `saturating_sub`。
- **（Medium）`auth_commands.rs:181-221`**: is_reauth チェック→重複チェック→挿入が別々のロック取得で TOCTOU。→ 1ロックスコープに。
- **（Medium）`cache_attachments`（`attachment_commands.rs:63-102`）**: 旧キャッシュ DELETE → ファイル書き込み → insert が非原子的。失敗時に古いキャッシュを失う。→ トランザクション + 失敗時巻き戻し。
- **（Medium）`directory_commands.rs:70-79` 連続する2回の DB ロック取得**: classifier 構築のためロック→解放→llm_provider 設定読取で再ロック。間に別スレッドの書き込みが割り込むと classifier と cloud 判定が別状態を見る余地。→ 1ロックスコープに統合。

#### アーキテクチャ（最重要）

- **（High）`mail_commands.rs:176-458` に同期ドメインロジック約280行が直書き**: 「INBOX同期→フラグ再同期→Sent同期」のオーケストレーション、`MergeStrategy` 選択、watermark 使い分け、Sent フォルダ探索フォールバック。→ `mail_sync/sync_service.rs`（新設）へ移動し、コマンドは SyncLocks 制御+進捗 emit+サービス呼び出しのみに。**これだけで mail_commands.rs（909行）は約200行の薄いアダプタになる。**
- **（High）`plan_delete`/`plan_archive` が `mail_commands.rs` と `bulk_commands.rs:45-64` に完全重複**、さらに `flag_commands.rs:12` `is_local_only_folder` も同じ「Sent はローカルのみ」判定。**同一ドメイン知識が3ファイルに分散**（bulk_commands.rs:35-42 のコメント自身が「意図的な重複」と自認）。→ サーバー反映ポリシーを1モジュール（`mail_policy`）に集約。ブランチ統合後の最優先リファクタ。
- **（Medium）`classify_commands.rs:63-79`** 分類結果の永続化判断 → `ClassificationService::apply_result` へ抽出。
- **（Medium）`auth_commands.rs:145-231`** OAuth フロー全体のオーケストレーション（コード交換→ID token→重複判定→保存→補償削除）→ `oauth::complete_oauth` へ。
- **（Medium）単体/一括の処理骨格重複**: `bulk_commands` の `delete_one`/`archive_one` と単体 `delete_mail`/`archive_mail` がほぼ同一フロー。→ 内部関数に共通化し両方から呼ぶ。
- **（Medium）IMAP 接続定型の3重複**（`mail_commands.rs:303-310, 430-437, 505-512`）→ `connect_account(account, secure_store)` に一本化。`list_attachments` と `get_inline_images` の取得フロー重複も同様（→ `fetch_and_cache_attachments` に集約）。
- **（Medium）`let conn = state.0.lock().map_err(AppError::lock_err)?` が20箇所以上**。→ `DbState::with_conn(f)` ヘルパ。
- **（Low）`flag_commands.rs:76-140`** `push_flagged`/`push_unseen_flag` の重複、進捗イベント struct の重複（`SyncProgressEvent`/`BackfillProgressEvent`）。

#### パフォーマンス

- **（Medium）`get_threads`（`mail_commands.rs:712-713`）**: フォルダ内全メールをロードしてスレッド化。数万件規模で表示のたびに全ロード。→ 将来ページネーション or SQL 集約。
- **（Medium）`inline_image_commands.rs:27-40`**: 全インライン画像を同時に base64 展開（元サイズの1.33倍を全保持）。→ サイズ上限・cid 単位の遅延取得を将来検討。

### 3.3 mail_sync / classifier

#### バグリスク

- **（Medium）`imap_client.rs:107-115` XOAUTH2 認証失敗時**: SASL のエラーチャレンジに空応答を返さず同じ auth 文字列を再送。サーバーによってはハング（15秒タイムアウトで落ちる）。→ process 呼び出し回数を状態化し2回目以降は空応答。
- **（Medium）`imap_client.rs:57-69` `connect_plain` の `login()` にタイムアウトなし**（XOAUTH2 側は15秒あり）。→ `tokio::time::timeout` で包む。
- **（Medium）`imap_client.rs:534-542` 非 UIDPLUS フォールバックの `expunge()`**: `\Deleted` 付き全メッセージを削除するため、他クライアントが付けたフラグを巻き込むリスク。Gmail は UIDPLUS 対応で回避されるが汎用 IMAP で危険。→ 検証 or 警告ログ。
- **（Low）`oauth.rs:375` `percent_decode`**: 末尾に `%X`（2文字残）があると `%` をリテラル出力しエラーパスに乗らない。→ 不正エンコーディングは明示的に Err。
- **（Low）`imap_client.rs:579` `copy_message`**: 1回目 COPY 失敗のエラー内容を捨てて CREATE→再試行。→ 両エラーを保持。
- **（Low）`ollama.rs:124` フォールバックエラー文の `&content[..100]`**: マルチバイト LLM 応答で char 境界外スライスによりパニックし得る。→ `chars().take(100)` に（分類 4 実装共通のフォールバックヘルパ集約と同時に）。

#### 重複（このモジュール群の最重要リファクタ）

- **（High）classifier 4実装の `classify` 本体がほぼ完全重複**（`claude.rs:128-147`, `claude_vertex.rs:172-191`, `gemini_vertex.rs:203-222`, `ollama.rs:107-127`）: `build_user_prompt → chat → parse → フォールバック` が同一。各実装は既に `generate_text` を持つため、**`classify` を trait のデフォルトメソッド化すれば4実装から丸ごと削除できる**（各約30行 → 0行）。プロバイダ追加のたびに増える構造的負債の解消。
- **（Medium）Anthropic レスポンス型（`MessagesResponse`/`ContentBlock`/`extract_text`）が claude.rs と claude_vertex.rs で完全重複** → `anthropic_common.rs` に括り出し。
- **（Medium）Vertex 共通処理**（endpoint host 分岐 + access_token 取得）が claude_vertex/gemini_vertex で重複 → `vertex_common.rs`。
- **（Low）reqwest::Client ビルダー（30s timeout）が4実装で重複** → `build_http_client()` を mod.rs に。
- **（Low）IMAP `list()`→属性フィルタが3関数で重複**（find_trash_folder/find_sent_folder/list_folders）。

#### セキュリティ（確認結果: 良好）

- SecureStore は平文保存なし、全ファイルでログへの機密出力なしを確認。OAuth id_token の署名検証省略は HTTPS 直取得なので妥当。`is_cloud_allowed` のフェイルクローズ・`..` セグメント防御・ユーザー編集欄不可侵の upsert は堅牢（模範的）。将来課題: 鍵のプロセスメモリ寿命（zeroize 検討、Low）。

#### パフォーマンス

- **（Medium）`imap_client.rs:281-287`**: バッチ内全メール本文を `Vec<FetchedMail>` に同時展開（SYNC_BATCH_SIZE=100）。大容量添付混在でピークが跳ねる。→ 本文サイズで動的バッチ分割 or 添付オンデマンド化（基盤は既にある）。
- **（Low）`fetch_flag_map` の `1:*` 全 UID 走査**（CONDSTORE 未対応）、`smtp_client.rs:82` `html_to_plain` の O(n²)。

#### dead code / 空テスト

- `oauth.rs:188` `TokenResponse.token_type` 未使用。
- `imap_client.rs:722,728` `test_auth_type_routing_*` は enum の型を確認するだけの空テスト（ルーティングを検証していない）。削除 or 実質化。
- `flag_name` の `MayCreate => "\\*"` は到達しない分岐。

---

## 4. フロントエンド詳細

### 4.1 バグリスク

- **（High）`AccountForm.tsx:107-113` OAuth 成功後にフォームが閉じない**: `handleOAuthCallback` が成功時にステータスを `"idle"` に戻すが、初期値も `"idle"` のため「未開始」と「成功」を区別できず、成功メッセージが出たまま UI が残る。→ `OAuthStatus` に `"success"` を追加し、AccountForm 側で完了時にフォームクローズ。
- **（High）`useMailDrag.ts:38-45`**: ドラッグ成立後、ドロップ対象外でマウスアップした場合に `endDrag()` が呼ばれない疑い（ゴースト残留・次クリック誤作動）。ドロップ側（ProjectListItem の onMouseUp）と突き合わせて、非成立時に必ず `endDrag()` する保証を入れる。
- **（Medium）`ThreadList.tsx:32-47` stale closure**: アカウント高速切替時に古い `fetchThreads` 結果が新しい一覧を上書きし得る。→ エフェクト内 cancelled フラグ。
- **（Medium）`UnclassifiedList.tsx:38-48`**: 2つの useEffect が初回マウントで `fetchUnclassified` を二重発火。→ 分類完了エッジ（true→false 遷移）のみ検出。
- **（Medium）`useProjectRename.ts:25-31`**: Enter submit と blur の二重発火 window（`await updateProject` 完了前に blur が走ると二重 invoke）。→ 送信中フラグでガード。
- **（Medium）`CloudSettingsDialog.tsx:98-117`**: トグル連打時のインフライト管理なし（古い rules スナップショットで楽観計算が競合）。→ 処理中 disabled。
- **（Medium）`RichTextEditor.tsx:36-40`**: `getHTML() !== value` 比較による setContent が、非正規化 HTML で再入し得る。→ 自分が出力した HTML を ref 保持して比較。
- **（Medium）`MailBody.tsx:42`**: DOMPurify サニタイズは良いが、リンクに `rel="noopener noreferrer"`/`target` 制御なし（タブナビング・アプリ内遷移）。→ `afterSanitizeAttributes` フックで強制 or クリックを `openUrl` にフック。
- **（Medium）`ProjectForm.tsx:69-82` / `ProjectTree.tsx:107-119`**: Tauri dialog `open()` が try/catch 外で未処理 rejection の可能性。
- **（Low）`MailBody.tsx:15-19`**: `useState(bodyHtml)` 初期値+effect 再セットの二重で、メール切替時に前メールの HTML が一瞬残り得る。→ `key={mail.id}` で再マウント。
- **（Low-Medium）`ThreadList.tsx:41-45`**: `syncAccount(...).then(() => fetchThreads(...))` は `needsReauth` で早期 return した場合でも `fetchThreads` が走る。→ 再認証が必要なときはスキップ。
- **（Low）`errorStore.ts:32-43`**: `addToast` の 5秒 setTimeout の id を保持せず、手動 dismiss 後もタイマーが残る（無害だがリーク）。→ id→timer の Map で `clearTimeout`。

### 4.2 パフォーマンス（最優先: セレクタなし全体購読）

`useMailStore()` 等をセレクタなしで呼ぶ全体購読が **ThreadList.tsx:22 / MailView.tsx:9 / UnclassifiedList.tsx:26（部分）/ Sidebar.tsx:24-39 / ProjectTree.tsx:19-21,86 / ComposeModal.tsx:27-44** に散在。`syncProgress`/`unreadCounts`/`backfillProgress` が同期中に高頻度更新されるため、**同期中は一覧・本文・サイドバーが再描画され続ける**。個別セレクタ化が費用対効果最大のパフォーマンス改善。

特に `ProjectTree.tsx:21` は `unclassifiedMails` の length しか使わないのに mailStore 全体を購読しており、**アプリ中のあらゆるメール操作でサイドバー全体が再描画**される。

その他:
- `ProjectListItem`/`ThreadItem` は memo 済みだが、親から渡す `onSelect`/`onClick` がインライン生成、`mailIds` も毎レンダー新規配列（`ThreadList.tsx:132`）で memo が効かない → useCallback / useMemo 化。
- `ProjectTree.tsx:97-104` のドロップは `moveMail` を1件ずつ直列 await（`bulkMoveMails` があるのに未使用。途中失敗も個別に握り潰され部分適用のまま無通知）。
- **`mailStore.ts:292-293`**: メール選択のたびに `mark_read` → 成功で `fetchUnreadCounts` を invoke。j/k 高速移動で invoke が連射される → デバウンス or `mark_read` の戻り値にカウントを含める。
- `MailBody.tsx:42` / `SearchResults.tsx:54`: `DOMPurify.sanitize` が毎レンダー実行 → `useMemo` でメモ化。

### 4.3 dead code

- **各ストアの `error: string | null` が完全な dead state**: mailStore / projectStore / accountStore / classifyStore が多数の catch で `set({ error })` するが、**error を読むコンポーネントはゼロ**（エラー表示はすべて errorStore トースト経由）。→ フィールドと全 `set({ error })` を削除。安全な純減で、無用な再レンダリングも消える。
- **`classifyStore.classifyMail`（公開単発分類メソッド）**: 呼び出しゼロ（実際の分類は `classifyAll`→`classifyNext` 経由）。→ 削除 or 将来予定をコメント明示。
- **`ClassifyResultBadge.tsx`**: コンポーネント本体+Props+専用テストがまるごと未配線（プロダクション参照ゼロ）。
- **`types/classifier.ts:11` `ClassifySummary`**: 参照ゼロ（ts-prune とも一致）。
- ClassifyResultBadge / ClassifySummary は「分類結果 UI」の未配線残骸の疑い。設計書と突き合わせて、配線するか削除するかを決める。

### 4.4 重複・共通化

- **invoke+エラー処理の定型が全ストア・複数ダイアログにコピペ** → `src/api/` 層（S1）で根治。
- **一括操作ハンドラ**（delete/archive/move + confirm 文言）が ThreadList.tsx:59-90 と UnclassifiedList.tsx:62-91 でほぼ同一 → `useBulkActions(threads, reload)` フックに抽出。
- **モーダルオーバーレイ**が MergeProjectDialog / CloudSettingsDialog / LlmSettingsDialog の3ダイアログで重複、ESC クローズ・フォーカストラップ・`aria-modal` も未統一 → `common/Modal.tsx` 新設。
- `CloudSettingsDialog` の `reload`/`refreshRules` 重複、`AccountList.tsx:73-89` の JSX 内 IIFE、`ProjectTree.tsx:179-207` のインライン IIFE ダイアログ、削除ボタン SVG のインライン肥大（→ アイコンコンポーネント化）。
- `ComposeMode` 型が `utils/composePrefill.ts` 定義のまま複数箇所から import（規約では `types/` に集約）。
- フォルダ名 `"Archive"`/`"INBOX"` のマジックストリング散在（`MailActions.tsx:17`、mailStore）→ 定数化。

### 4.5 状態管理設計

- **mailStore（546行）のゴッドストア化**: threads/selection/sync/unread/backfill/bulk/unclassified + ツリー操作ヘルパ15個 + イベントリスナー配線。旧 L-4 の classifyStore 過多は「解消」ではなく mailStore への「移動」だった。→ S6 で syncStore/unclassifiedStore/bulkActionStore に分割。
- **`unclassifiedMails` と `unclassifiedThreads` の二重管理**（`mailStore.ts:31-33,411-414`）: 同一データのフラット版とスレッド版を別 state で保持し、全 mutation（markRead/setFlagged/markUnread/removeMailFromState 等）が両方を手作業で並行更新。片方の更新漏れが将来バグになりやすい構造。→ スレッド版を単一の真実とし、フラット版はセレクタで導出。
- ストア間連携が `getState()` 直呼びの暗黙オーケストレーション（classifyStore→projectStore、composeStore→accountStore/draftStore、mailStore→accountStore/uiStore）。→ オーケストレーションは上位のユースケース層/フックへ。
- サーバー状態（threads/unread）と UI 選択状態（selectedThread/selectedMail）が mailStore に混在（複数選択の selectionStore は分離済みなのに単一選択は内包）。
- イベントペイロード型・invoke 戻り型のローカル定義（`SyncProgress`/`NewMailEvent`/`BackfillOutcome`/`UnclassifiedMailRef`/`ComposeAttachment`）→ api 層 or `types/` に集約。
- エラーハンドリングの不統一: local error+toast / toast のみ / console.error のみが混在。

---

## 5. 無駄なコードコメント

全体としてコメントは「なぜ」を説明する良質なものが多く（トゥームストーン理由、UIDPLUS 回避、正規化理由、設計書参照など）、**AI 生成的な自明コメントは少ない**。削除候補は限定的:

| 場所 | 内容 |
|------|------|
| `bulk_commands.rs:35-42` | 8行の「意図的な重複」説明 → 共通化タスク化して1行 TODO に縮約（重複解消と同時に削除） |
| `classify_commands.rs:15-17, 27-29` | `// State types` / `// Tauri commands` のセクション見出し |
| `classify_commands.rs:208` | `// --- get_mail_by_id (now in db::mails) ---` 移動履歴コメント |
| `directories.rs:21-24` 前半 | コードを逐語的になぞる部分（「なぜ」の後半は残す） |
| `AccountForm.tsx:9` | `// 10 minutes`（定数名で自明） |
| `NotificationToggle.tsx:9-16` | localStorage 仕様説明が notifyNewMail.ts と重複（片方に集約） |
| `classifyStore.ts:44-47` | serde flatten 事情の4行解説（Rust側実装詳細。1行に圧縮可） |
| `mailStore.ts:224-227` | backfill 多重実行ガードの4行説明（syncAccount 側と重複） |
| `dragStore.ts:5,9` / `useMailDrag.ts:6-9` | フィールド名・シグネチャと同義のコメント |
| `migrations.rs:250, 258` | トリガー名と同義のコメント（`INSERT OR REPLACE triggers DELETE then INSERT` の補足は残す） |
| `date.ts` | コメント言語だけ英語（統一するなら日本語へ） |

---

## 6. dead code 一覧

| 場所 | 内容 | 対処 |
|------|------|------|
| `src/components/common/ClassifyResultBadge.tsx` | コンポーネント+テストがまるごと未配線 | 配線 or 削除（設計書と突き合わせ） |
| `src/types/classifier.ts:11` `ClassifySummary` | 参照ゼロ | 同上 |
| `oauth.rs:188` `TokenResponse.token_type` | 読み出しゼロ | 削除 |
| `imap_client.rs:722,728` `test_auth_type_routing_*` | 型を確認するだけの空テスト | 削除 or 実質化 |
| `imap_client.rs` `flag_name` の `MayCreate => "\\*"` | 到達しない分岐 | 削除 |
| `assignments.rs:84-94` `add/remove_follow_exclusion` | 外部呼び出しなし（内部+テストのみ） | private 化 |

Rust 側の未使用関数・未登録コマンドは grep で全数確認し**検出なし**（全56コマンドが `lib.rs` に登録済み）。

---

## 7. 段階的リファクタリング計画（PR ロードマップ）

GitHub Flow / Single Concern の規約に沿い、1 PR = 1 関心事で並べる。**フェーズ0（バグ修正）とフェーズ1以降（構造改善)は独立して進められる。**

### フェーズ0: バグリスク解消（すぐやる・小さいPRの束）

> **着手順の考え方**: まず 0-2（マイグレーション tx 化）と 0-10（テストの FK ON 統一）を「土台」として先行させる。整合性の再発防止をテストが検証できる状態を作ってから、各書き込み系の tx 境界修正（0-1, 0-4〜0-6）に進むと、修正の効果をテストで担保できる。

| PR | 内容 | 対応指摘 |
|----|------|---------|
| 0-1 | db層のトランザクション化（upsert_sent_mail / approve_classification / move_mail_to_project / delete_account） | B-1, B-2, B-4 |
| 0-2 | マイグレーションの1バージョン1トランザクション化 + ディスパッチのテーブル化 | B-5 |
| 0-3 | `filter_map(r.ok())` → `collect::<Result<_>>()?` 一括置換 + `.unwrap_or(0)` 系の `optional()` 化 | B-10 |
| 0-4 | send_mail の uid 採番原子化 + 送信後ローカル反映失敗のベストエフォート化 | B-6, B-7 |
| 0-5 | save_attachment パス検証 + 添付キャッシュ孤児化の解消 | B-8, B-9 |
| 0-6 | auto_follow_threads の N+1 解消（assignments 先読み + 軽量 SELECT + トランザクション） | B-3 |
| 0-7 | フロント: OAuth success ステータス追加でフォームクローズ / useMailDrag の endDrag 保証 | B-11, B-13 |
| 0-8 | フロント: Zustand セレクタ化（ThreadList / MailView / UnclassifiedList / Sidebar / ProjectTree / ComposeModal）+ dead な error state 削除 + UnclassifiedList 二重発火解消 + ドロップの bulkMoveMails 化 | B-12 |
| 0-9 | clippy --fix + PendingClassifications リーク経路の除去 + auth_commands の saturating_sub/read ループ | — |
| 0-10 | `test_helpers::setup_db` に `PRAGMA foreign_keys = ON` を追加（各テストの手動 pragma を一元化） | B-14 |

### フェーズ1: 重複解消（構造改善の前哨）

| PR | 内容 |
|----|------|
| 1-1 | classifier `classify` の trait デフォルトメソッド化（4実装から削除）+ フォールバックヘルパ集約（ollama.rs:124 のバイトスライスも同時修正） |
| 1-2 | anthropic_common / vertex_common の括り出し |
| 1-3 | サーバー反映ポリシー（plan_delete/plan_archive/is_local_only_folder）を `mail_policy` モジュールに集約、単体/bulk の処理本体共通化 |
| 1-4 | db層: `Model::from_row` 統一 + MAIL_COLUMNS 一元化 + mark_read/unread/set_flagged 共通化 + `DbState::with_conn` |
| 1-5 | フロント: `common/Modal.tsx` + `useBulkActions` + アイコン抽出 + フォルダ名定数化 |

### フェーズ2: 層の確立（Clean Architecture 化）

| PR | 内容 | 対応 |
|----|------|------|
| 2-1 | **`src/api/` 新設**（invoke 集約・arg 変換・エラー正規化・reauth を型で表現）。stores は api を呼ぶだけに | S1 |
| 2-2 | **バッチ分類をバックエンドの `classify_batch` ユースケースに戻す**（進捗イベント + 1コマンド化） | S2 |
| 2-3 | ThreadList の直 invoke を mailStore アクションへ | S3 |
| 2-4 | 同期ロジックを `mail_sync/sync_service.rs` へ移動（mail_commands を薄いアダプタに） | §3.2 |
| 2-5 | `MailRepository` trait 化（委譲実装で非破壊）→ Project/Assignment へ展開 | S4 |
| 2-6 | `ClassificationService::classify_one` 抽出（確信度閾値をサービスへ移動、commands層テスト開始） | S5 |
| 2-7 | `build_threads` 系をドメイン `threading.rs` へ / db/mails.rs 分割（sent_sync 分離） | §3.1 |

### フェーズ3: DDD 本格化（必要になったら）

- mailStore 分割（S6）、Project 集約確立（S7）、models へのドメイン振る舞い移動（S8）、domain/usecases/ports/adapters ディレクトリ再編（§2.5）。

---

## 8. 過去レビュー（2026-04-14）の追跡結果

| 項目 | 状態 |
|------|------|
| H-1 XSS（DOMPurify） | ✅ 解消（MailBody.tsx:42, SearchResults.tsx:10 で確認） |
| H-2 ドラッグ重複 | ✅ 解消（useMailDrag.ts に集約）— ただし B-13 の endDrag 問題は新規指摘 |
| H-3 直接 setState | ✅ 解消（setThreads アクション化）— ただし ThreadList の直 invoke が残渣 |
| M-2 クライアント側スレッド合成 | ✅ 解消（get_threads_by_project / get_unclassified_threads） |
| M-3 classify_unassigned のロック競合 | ⚠️ **形を変えて残存** — バックエンドから消えフロント classifyStore.classifyNext へ移設。業務ロジックの UI 層漏れという新たな歪みに（→ PR 2-2） |
| M-4 CSP 未設定 | ✅ 解消（tauri.conf.json:28） |
| M-5 未使用 useEffect | ✅ 解消 |
| M-7 viewMode | ✅ 解消（uiStore） |
| M-9 ollama expect() | ✅ 解消（本体コードの expect はほぼゼロを確認） |
| M-11 手動 BEGIN/COMMIT | ✅ 解消（conn.transaction()） |
| L-3 commands層テスト 0件 | ⚠️ 残存 — 根本原因はユースケースが Tauri に貼り付いていること（→ PR 2-6 で解消の道が開ける） |
| L-4 classifyStore 責務過多 | ⚠️ **移動しただけ** — 未分類一覧・moveMail が mailStore へ移り、mailStore がゴッドストア化（→ フェーズ3） |

---

## 9. 良かった点（維持すべき設計）

- `classifier` の trait port と `rescan_project` の依存注入 — Clean Architecture の実例として既に存在。横展開の手本。
- `mail_sync` の純関数分離（plan_batches / html_to_plain / build_message）と idle.rs の依存注入テスト。
- `project_context` の責務分離（scanner/extractor/digest/cloud_policy/context_file）とフェイルクローズのクラウド送信ポリシー。模範的。
- FTS5/LIKE のサニタイズ（`sanitize_fts_query` / `escape_like`）とテスト網羅。
- `merge_projects` のトランザクション使用 — db層の書き込みが従うべき手本。
- 設計判断の「なぜ」を残すコメント文化と設計書参照（`2026-07-12-sent-sync-uidplus-design.md「C1」` 等）。
- SecureStore の暗号化保存、機密のログ出力ゼロ。
