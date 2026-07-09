# 案件ディレクトリ連携（Project Directory Context） 設計書

## 概要

案件（project）にローカルディレクトリを紐付け、そこに保存されたファイル（図面、香盤表、スケジュール、メモ等）から案件のコンテキストを抽出して、メール分類の精度を高める機能。

### 背景

Pigeon の主ユーザーは舞台監督。複数の舞台（案件）を並行して準備し、会場管理者・演者・美術業者など多様な相手とメールをやり取りする。案件ごとの資料は必ずローカルPCのディレクトリに整理されているため、そのディレクトリを案件のコンテキスト源として使えば、「このメールはどの舞台の件か」の判定材料が大幅に増える。

### スコープ

**含む:**

- 案件⇔ディレクトリの手動紐付け（作成フォーム + 右クリックメニュー）
- ディレクトリのスキャン（起動時 + 手動再スキャン）とファイルインベントリの管理
- ファイル名・フォルダ構造（全ファイル）+ テキスト系ファイル（.txt / .md 等）の内容抽出
- 案件コンテキストファイル `PIGEON-CONTEXT.md` の生成・自動更新
- 分類プロンプトへのコンテキスト注入
- クラウドLLM利用時の送信可否コントロール（案件・ディレクトリ・ファイル単位）

**含まない（将来リスト）:**

| 機能 | 備考 |
|------|------|
| 返信内容の提案 | メール作成機能（SMTP送信・エディタ）の実装後。`PIGEON-CONTEXT.md` とコンテキスト基盤はそのまま入力として再利用する |
| PDF / Office ファイルの内容抽出 | `content_kind` 列挙で拡張余地を確保済み。抽出結果キャッシュが必要になったら `project_file_texts` テーブルを追加する |
| 親フォルダ登録によるAIのディレクトリ候補提案 | AIが新案件を作ったとき、名前の似たサブフォルダを紐付け候補として提案する |
| ファイルウォッチャーによる即時反映 | notify 系クレート。当面は起動時 + 手動で十分 |
| 添付ファイルの案件ディレクトリへの保存 | メーラー→ディレクトリ方向の連携 |

## 1. アーキテクチャ

### データフロー

```
案件にディレクトリを紐付け（手動）
    │
    ▼
スキャン（起動時バックグラウンド + 手動「再スキャン」）
    ├─ ディレクトリ走査 → ファイルインベントリを DB に保存
    │   （相対パス・サイズ・mtime・テキスト系は内容ハッシュ）
    ├─ テキスト系ファイルの内容を抽出（上限付き、保存はしない）
    └─ inventory_hash（構成全体のハッシュ）を計算
    │
    ▼ inventory_hash が前回と異なる案件のみ
PIGEON-CONTEXT.md の auto セクションを LLM で再生成
    ├─ 入力: ファイルツリー + テキスト内容（送信可否ポリシー適用済み）
    ├─ 出力: 公演名・会場・関係者・業者・キーワード等の要約
    └─ マーカーより上（ユーザー自由記入欄）は不可侵
    │
    ▼
DB キャッシュ更新（project_contexts.cached_context）
    │
    ▼
メール分類時
    既存プロンプト（案件名 + 説明 + 最近の件名3件 + 修正履歴）に
    各案件の cached_context を追加注入（1案件800字上限）
```

### 設計原則

- **分類はホットパス**: 分類時にはディレクトリ I/O も LLM 追加呼び出しも行わない。読むのは DB キャッシュのみ。重い処理（走査・抽出・ダイジェスト生成）はすべてスキャン時に済ませる
- **人間が触るものはファイル、アプリ内部の状態は DB**: コンテキストの正本はユーザーが読める・編集できる `PIGEON-CONTEXT.md`。インベントリ・送信可否ルール・キャッシュは DB
- **クラウド送信はデフォルト拒否**: 許可判定は「明示的に許可ルールにマッチした場合のみ true、曖昧なら false」。危険側に倒れない

### Rust モジュール構成

新モジュール `project_context/` を追加:

| ファイル | 責務 |
|---------|------|
| `scanner.rs` | ディレクトリ走査・インベントリ差分適用・inventory_hash 計算 |
| `extractor.rs` | テキスト系ファイルの内容抽出（上限適用） |
| `context_file.rs` | PIGEON-CONTEXT.md の読み書き・マーカー処理 |
| `digest.rs` | LLM によるダイジェスト（auto セクション）生成 |
| `cloud_policy.rs` | クラウド送信可否の判定 |

既存への変更は最小限:

- `classifier/prompt.rs`: 案件ごとのコンテキスト文字列を受け取って注入する変更のみ
- `db/`: 新テーブルの CRUD（`directories.rs`, `project_files.rs`, `cloud_rules.rs`, `project_contexts.rs`）
- `commands/`: `directory_commands.rs` を追加

## 2. データモデル（migrate_v5）

```sql
-- 案件⇔ディレクトリ (1:N。UIは当面1案件1ディレクトリに制限)
CREATE TABLE project_directories (
    id              TEXT PRIMARY KEY,          -- uuid v4
    project_id      TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    path            TEXT NOT NULL UNIQUE,      -- 絶対パス。二重紐付けをDBで防止
    is_primary      BOOLEAN NOT NULL DEFAULT FALSE,  -- PIGEON-CONTEXT.md の置き場所
    status          TEXT NOT NULL DEFAULT 'ok'
                    CHECK(status IN ('ok','missing','inaccessible','error')),
    last_scanned_at DATETIME,
    created_at      DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_project_directories_project ON project_directories(project_id);
CREATE UNIQUE INDEX idx_project_directories_one_primary
    ON project_directories(project_id) WHERE is_primary = TRUE;

-- ファイルインベントリ (現在の実体のスナップショット。消えたファイルはハードデリート)
CREATE TABLE project_files (
    id             TEXT PRIMARY KEY,
    directory_id   TEXT NOT NULL REFERENCES project_directories(id) ON DELETE CASCADE,
    relative_path  TEXT NOT NULL,
    size_bytes     INTEGER NOT NULL,
    mtime          DATETIME NOT NULL,
    content_hash   TEXT,               -- 抽出対象(テキスト系)のみ。他は NULL
    content_kind   TEXT NOT NULL DEFAULT 'none'
                   CHECK(content_kind IN ('none','text','pdf','office','other')),
    extract_status TEXT NOT NULL DEFAULT 'ok'
                   CHECK(extract_status IN ('ok','skipped_too_large','unsupported','error')),
    indexed_at     DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(directory_id, relative_path)
);
CREATE INDEX idx_project_files_directory ON project_files(directory_id);

-- クラウド送信許可ルール (ファイル行への焼き込みではなくルールで表現)
-- 判定: 最長 relative_path マッチのルールが勝つ。マッチ無し = 不許可
CREATE TABLE project_cloud_rules (
    id            TEXT PRIMARY KEY,
    directory_id  TEXT NOT NULL REFERENCES project_directories(id) ON DELETE CASCADE,
    scope         TEXT NOT NULL CHECK(scope IN ('directory','file')),
    relative_path TEXT NOT NULL DEFAULT '',   -- '' = ディレクトリ全体
    allow         BOOLEAN NOT NULL,           -- false = 明示的除外(親の許可を打ち消す)
    created_at    DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(directory_id, scope, relative_path)
);
CREATE INDEX idx_project_cloud_rules_directory ON project_cloud_rules(directory_id);

-- 案件のAIコンテキスト状態 (正本は PIGEON-CONTEXT.md、これはキャッシュ+メタ)
CREATE TABLE project_contexts (
    project_id          TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    cached_context      TEXT,      -- 分類注入用 (800字に切詰済み)
    context_hash        TEXT,      -- md の auto セクションのハッシュ (外部編集検知→自己修復)
    inventory_hash      TEXT,      -- ファイル構成全体のハッシュ (O(1)で再生成要否判定)
    allow_cloud_context BOOLEAN NOT NULL DEFAULT FALSE,
    generated_at        DATETIME
);
```

### スキーマ設計の根拠

サブエージェント2系統（Codex / アーキテクト）による独立レビューの合議で決定した:

- **1:N + UI制限**: `project_id PRIMARY KEY`（1:1強制）だと複数ディレクトリ対応時に主キー変更のマイグレーションが必要になる。スキーマは広く、振る舞いは狭く
- **許可ルールテーブル**: ファイル行に `cloud_allowed` を焼き込むと、ディレクトリ許可後に追加されたファイルへ許可が伝播しないバグ（未許可ファイルの送信事故 or 意図した許可の漏れ）を生む。ルール方式ならユーザーの意図が正本で、実ファイルは毎回評価される
- **`project_contexts` 分離**: `projects` に TEXT カラムを足すとホットな案件一覧クエリに重いデータが混入する。コンテキスト関連の状態（キャッシュ・ハッシュ・送信可否）を1箇所に凝集
- **抽出テキストは保存しない**: 機微データの平文を SQLite に増やさない（データ最小化）。テキスト系の再抽出は十分安い。PDF 抽出導入時に `project_file_texts(file_id PK, extracted_text, ...)` を別テーブルで追加する
- **content_hash は抽出対象のみ**: 舞台案件のフォルダには巨大な図面 PDF・写真・動画が入り得るため、全ファイルのハッシュ計算はスキャンを不必要に重くする。非抽出ファイルは (パス+サイズ+mtime) で構成変化を検知
- **グローバル `UNIQUE(path)`**: 同じディレクトリを2案件に紐付けると、両案件が同じ PIGEON-CONTEXT.md の auto セクションを取り合って壊すため DB で防ぐ
- **`is_primary`**: 複数ディレクトリ解禁時に PIGEON-CONTEXT.md の置き場所が曖昧になる問題を部分ユニークインデックスで先回りして解決。UI が1案件1ディレクトリ制限の間は、紐付けたディレクトリを常に `is_primary = TRUE` で登録する
- **列挙は初手から広めに**: SQLite の CHECK 制約変更はテーブル再作成になるため、`status` に `inaccessible`/`error`、`content_kind` に `pdf`/`office` を先行定義

### 既存設計書との不整合（要更新）

レビュー中に発見: `2026-04-12-pigeon-design.md` のスキーマは実装（migrations.rs）より古い。実 DB では `projects` が `account_id`（migrate_v3、`ON DELETE CASCADE`）を持ち、`attachments` テーブルは未作成。本体設計書のデータモデル節を実装に合わせて更新すること。

## 3. PIGEON-CONTEXT.md

### 形式

案件のプライマリディレクトリ直下に置く。`<!-- pigeon:auto -->` マーカーより上がユーザー自由記入欄、下が AI 管理セクション。

```markdown
# 〇〇ホール 春公演

（ここから上は自由記入欄。AIは絶対に書き換えない。
 会場担当の連絡先、搬入時の注意、自分用メモなど何でも）

<!-- pigeon:auto -->
## 案件コンテキスト（自動生成 2026-07-09）

- 公演: △△バレエ団 春公演「くるみ割り人形」
- 会場: 〇〇ホール（キーワード: 平面図, 吊物, 重量制限）
- 関係する組織・人: △△バレエ団, □□舞台美術, ...
- 主なファイル: 平面図.pdf, 香盤表.md, 搬入スケジュール.txt, ...
```

### 更新規約

- **auto セクションは再スキャンのたびに AI が自動で追記・修正する**（inventory_hash の変化を検知したら全置換で再生成）
- ダイジェスト生成は現在選択中の LLM プロバイダで行う。クラウド LLM 選択時は送信可否ポリシー（§5 の層1）を適用した入力のみを使い、許可ファイルが1件も無い場合は生成をスキップして前回の auto セクションを維持する
- マーカーより上には何があっても書き込まない
- ファイルが既に存在する場合（ユーザーが自作していた場合）はそのまま尊重し、末尾にマーカー + auto セクションを追加
- マーカー欠損時は末尾に再追加。マーカーが複数ある場合は最初のマーカーを正とし、それ以降全体を auto セクションとして扱う
- 生成 LLM が失敗した場合は前回の auto セクションと DB キャッシュを維持する（劣化させない）

### キャッシュとの同期（自己修復）

正本はファイル、DB はキャッシュ。乖離は起動時スキャンで解消する:

1. 起動時に md を読み、auto セクションのハッシュを `project_contexts.context_hash` と照合
2. 不一致（= ユーザーが外部エディタで編集した等）なら `cached_context` を md の内容から再構築
3. md が消えていたら次回のダイジェスト生成時に再作成

## 4. スキャン仕様

| 項目 | 値 |
|------|-----|
| タイミング | アプリ起動時（バックグラウンド、全案件） + 案件右クリック「再スキャン」 |
| 最大走査ファイル数 | 2,000 / ディレクトリ |
| 最大深さ | 10 |
| テキスト抽出上限 | 1ファイル 10KB、案件全体 100KB |
| テキスト系の判定 | 拡張子ベース: .txt, .md, .csv, .json, .yaml, .yml, .html |
| 無視するもの | 隠しファイル・隠しディレクトリ、シンボリックリンク（ループ・案件外脱出の防止）、`node_modules` 等の定番除外 |
| 差分適用 | (relative_path, size_bytes, mtime, content_hash) で比較。消えたファイルはハードデリート |
| 再生成トリガー | inventory_hash（全ファイルの relative_path + size + mtime + content_hash を正規化して集約したハッシュ）の不一致 |
| トランザクション | インベントリ更新はディレクトリ単位。途中でアプリが落ちても次回冪等にやり直せる |

PIGEON-CONTEXT.md 自体はインベントリ・抽出対象から除外する（自己参照ループ防止）。

## 5. プライバシー / クラウド送信ポリシー

デフォルト LLM は Ollama（ローカル）だが、実運用ではローカル LLM の精度限界からクラウド LLM（Claude API 等）の利用が現実的な前提。よって送信可否を3層で制御する:

| 層 | 対象 | デフォルト | 制御するもの |
|----|------|-----------|-------------|
| 1. ファイル/ディレクトリ単位ルール (`project_cloud_rules`) | ダイジェスト生成の入力 | 不許可 | クラウド LLM で生成する際、許可されていないファイルは**名前も内容も**プロンプトに含めない |
| 2. 案件単位フラグ (`allow_cloud_context`) | 分類時の注入 | 不許可 | PIGEON-CONTEXT.md の内容をクラウド LLM の分類プロンプトへ注入してよいか。中身はユーザーが目で確認できるファイルなので「読んで納得したものだけ ON」という運用ができる |
| 3. Ollama（ローカル）選択時 | すべて | 許可 | データがマシンから出ないため無条件で利用可 |

### 不変条件（テストで担保）

1. クラウド LLM へのプロンプト構築経路で、許可ルールに明示的にマッチしないファイルは名前すら含めない
2. `allow_cloud_context = false` の案件のコンテキストはクラウド LLM に注入しない
3. 判定関数は曖昧な場合（ルール不在・パス不一致・状態不明）常に `false` を返す
4. ルール評価は最長 relative_path マッチ優先。明示的 `allow = false`（除外）が親ディレクトリの許可に勝つ

### agent.md セキュリティルールの改訂

現行の「LLMへ送信するデータは件名、送信者、本文冒頭300文字に限定する」を以下に改訂する:

> LLMへ送信するデータは、件名・送信者・本文冒頭300文字、および案件ディレクトリ連携のコンテキスト（本設計書の送信可否ポリシーに従う）に限定する。クラウドLLMへのファイル由来データの送信はユーザーが明示的に許可したものに限る。

## 6. 分類プロンプトへの注入

- 既存の案件サマリー（名前 + 説明 + 最近の件名3件）に `project_contexts.cached_context` を追記する
- 1案件あたり 800 字上限。超過時はユーザー自由記入欄を優先し、auto セクション側から切り詰める
- クラウド LLM 使用時は `allow_cloud_context = true` の案件のみ注入。Ollama は全案件注入
- コンテキストが無い案件（未紐付け・未生成）は従来どおりの形式（後方互換）

## 7. UI

新ペインは作らず、既存 UI への追加のみ。

| 場所 | 追加内容 |
|------|---------|
| 案件作成/編集フォーム (`ProjectForm`) | 「フォルダを紐付け」ボタン（Tauri ネイティブフォルダ選択ダイアログ）。任意項目 |
| 案件右クリックメニュー (`ContextMenu`) | 「フォルダを紐付け/変更」「再スキャン」「クラウド送信設定…」「紐付け解除」 |
| サイドバー案件行 (`ProjectListItem`) | 紐付け済みは 📁 アイコン。`status != 'ok'` は ⚠（ホバーで理由表示）。スキャン中は ⏳ |
| サイドバー下部 (`ScanIndicator`) | スキャン中の案件名を表示（実装時の適応: アプリにステータスバーが存在しないため、当初案の「ステータスバー」からサイドバー下部のインジケータに変更） |

### クラウド送信設定ダイアログ（新規コンポーネント）

- ファイルツリーをチェックボックス付きで表示。ディレクトリのチェックは配下に適用（`scope='directory'` ルール）、個別ファイルのチェック/解除は上書きルールとして保存
- ツリー上部に案件単位トグル「コンテキストファイルをクラウドLLMへ送信する」。ON にする際、PIGEON-CONTEXT.md の現在の中身をプレビュー表示して確認させる
- Ollama 選択中は冒頭に「現在ローカルLLM使用中のためデータは外部送信されません」と表示（設定自体は保存され、クラウド切替時に効く）

### 状態管理

新ストアは作らず `projectStore` に `directories` / `scanStatus` を追加。

## 8. エラーハンドリング

| 状況 | 挙動 |
|------|------|
| ディレクトリ不在（外付けHDD未接続・移動済み） | `status='missing'`。スキャンのみスキップ、**キャッシュ済みコンテキストは分類に使い続ける**。紐付けデータは消さない。UI に ⚠ |
| 権限エラー | `status='inaccessible'`。同上 |
| 個別ファイル読み取り失敗 | `extract_status='error'` で記録して続行。スキャン全体は失敗させない |
| PIGEON-CONTEXT.md の破損（マーカー重複等） | 最初のマーカーを正とする。マーカー消失時は末尾に再追加。ユーザー欄は何があっても不可侵 |
| ダイジェスト生成の LLM 失敗 | 前回の auto セクションとキャッシュを維持。リトライは次回スキャン時 |
| スキャン中のアプリ終了 | ディレクトリ単位トランザクションで冪等にやり直し可能 |

## 9. テスト戦略（TDD）

### Rust（ユニット中心）

- `scanner`: tempdir フィクスチャで、インベントリ差分（追加/削除/リネーム/mtime変更）、上限（ファイル数・深さ・サイズ）、シンボリックリンク無視、inventory_hash の安定性
- `context_file`: マーカー分割・auto セクション置換・ユーザー欄不可侵・マーカー欠損/重複の復旧（純粋関数、テーブルテストで網羅）
- `cloud_policy`: 最長マッチ判定、明示除外が親許可に勝つ、ルール不在→不許可（危険側のテストを厚く）
- `classifier/prompt`: コンテキスト注入あり/なし、800字切詰、クラウド時の除外
- `digest`: モック LLM（既存 `test_helpers` を流用）

### React（Vitest + RTL）

- クラウド送信設定ダイアログ: チェックのカスケード表示、トグル ON 時のプレビュー確認フロー
- `ProjectListItem` の 📁 / ⚠ 表示、右クリックメニュー項目

### 統合（`tests/`）

tempdir 作成 → 紐付け → スキャン → PIGEON-CONTEXT.md 生成（モックLLM）→ ファイル追加 → 再スキャンで auto セクションのみ更新（ユーザー欄が無傷）までのエンドツーエンド。

## 10. 実装フェーズ（PR 分割の目安）

1. DB マイグレーション v5 + CRUD（`db/directories.rs` 等）
2. スキャナ + インベントリ差分 + inventory_hash
3. PIGEON-CONTEXT.md 読み書き（マーカー処理）+ ダイジェスト生成
4. 分類プロンプトへの注入 + クラウド許可判定
5. UI（紐付け・再スキャン・⚠表示）
6. クラウド送信設定ダイアログ

各フェーズは Stacked PR として依存関係を明記する。
