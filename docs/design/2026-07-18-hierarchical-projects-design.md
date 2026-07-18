# 案件（Project）の階層化 設計書

- 作成日: 2026-07-18
- ステータス: レビュー中（Codex 3回レビュー済み・条件付きGo反映済み）
- 関連: `docs/design/2026-04-12-pigeon-design.md`（全体設計）、`docs/design/2026-04-13-phase2-ai-classification-design.md`（AI分類）、`docs/design/2026-07-09-project-directory-context-design.md`（案件ディレクトリ）、`docs/adr/0002-cloud-llm-data-boundary.md`、`docs/adr/0004-ai-native-dispatch-architecture.md`、`docs/design/2026-07-17-search-enhancement-design.md`（ベクトル検索）

## 1. 背景と目的

現状の案件（projects）はフラットでアカウント直下にしか作れない。ペルソナ「アスカ」（舞台照明スタッフ、`docs/personas.md`）の実務では「AKB48アリーナツアー > 埼玉スーパーアリーナ > 音響」のような階層で管理できる方が使いやすい。

本設計は案件を**深さ無制限の木構造**に変更する破壊的仕様変更である。個人開発であり、既存挙動の形式的互換より設計の健全性を優先する（`agent.md` 不具合修正方針）。

## 2. スコープ

### やること
- `projects` への `parent_id` 導入（隣接リスト + 再帰CTE、migration v19）
- 階層不変条件（循環禁止・同一アカウント・account_id 不変）のDBトリガー化
- 集約表示（親ノード選択で配下全メールをスレッド表示）
- 階層対応の操作系（子作成・付け替え・サブツリー削除/アーカイブ・マージ）の UseCase 化と dispatch バス移行
- AI分類の階層対応（パス付きツリー提示・最深確信ノード割当・create with parent）
- 設定の加算的継承（ディレクトリ・コンテキスト）
- サブツリー限定検索（FTS・セマンティック両方）
- `correction_log` のパススナップショット化（マージ・削除で学習履歴が壊れる既存バグの根治を含む）

### やらないこと（YAGNI）
- 案件→案件の D&D による付け替え（コンテキストメニュー経由のみ。D&D は将来拡張）
- 継承の除外・オーバーライド機能（加算のみ）
- アーカイブの復元操作（現行にも無い）
- 既存フラット案件の自動階層化提案（手動で set_parent すれば足りる）
- 検索スコープ切替以外の検索UI高度化（ベクトル検索UIは別設計）

## 3. 確定要件

1. **所属モデル**: メールは任意ノード（親でも葉でも）に直接所属できる。親ノード選択で配下全体を集約表示。ツアー全体向け連絡は「ツアー」ノード直属に置ける
2. **深さ**: 無制限（循環のみ禁止。人為的な深さ上限を設けない）
3. **AI分類**: LLM にパス付きツリーを提示し「確信できる最も深いノード」に割当（迷えば親に逃がす）。既存案件配下への子案件作成提案も可
4. **設定の継承（加算的）**: ノードNの有効コンテキスト = ルート→Nのパス上の全ノードのディレクトリ+コンテキストの合算。クラウド送信可否はマージせず「そのディレクトリを紐付けたノード」のルールに従う（ルール合成を発生させない。ADR-0002 の送信境界の監査を単純に保つ）
5. **サブツリー限定検索**: project_id→サブツリー展開フィルタを FTS とセマンティック検索の両方に追加。UI は「この案件内で検索」トグル
6. **DB表現**: 隣接リスト（parent_id 自己参照FK）+ 再帰CTE。個人メールクライアントの規模（案件数百・深さ実用3〜5）では再帰CTEで十分であり、マテリアライズドパス/クロージャテーブルの導出データ同期という失敗モードを持ち込まない。将来必要になればクロージャテーブルを導出キャッシュとして後付け可能

## 4. データモデル（migration v19）

前提: main は v18 まで消費済み（v18=ベクトル検索、v11 は予約欠番）。本件は **v19**（実装時に他ブランチが v19 を消費していたら次の空き番号に読み替え）。

### 4.1 projects への parent_id 追加

```sql
ALTER TABLE projects ADD COLUMN parent_id TEXT REFERENCES projects(id) ON DELETE CASCADE;
CREATE INDEX idx_projects_parent ON projects(parent_id);
```

- 既存案件は `parent_id = NULL`（＝ルート）のまま。データ書き換え不要で移行完了
- DEFAULT 未指定（=NULL）のため FK 有効時の ADD COLUMN 制限を満たす
- `ON DELETE CASCADE` は**防御層**。削除の正常経路は §5 の葉先行の明示削除。SQLite の FK CASCADE はトリガー再帰深度上限（既定1000）に服するため、深いサブツリーの再帰CASCADEを正常系にしない（深さ1105のルート先行削除が `too many levels of trigger recursion` で失敗、葉先行明示削除は成功することを SQLite 3.45.0 実機で確認済み）

### 4.2 階層不変条件のトリガー（DBを最終防衛線にする）

アプリ層（`set_parent` 等）でも同じ検証を行いユーザー向けエラーを返す。トリガーは将来の別 UseCase・修復スクリプト等の迂回経路に対する最終防衛線。bundled SQLite（rusqlite 0.31 = 3.45.0）はトリガー内再帰CTE使用可（3.34+）。

```sql
-- 循環禁止（UPDATE）: 新しい親が自分自身または自分の子孫なら拒否
CREATE TRIGGER trg_projects_no_cycle
BEFORE UPDATE OF parent_id ON projects
WHEN NEW.parent_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'project hierarchy cycle')
    WHERE NEW.parent_id = NEW.id
       OR NEW.parent_id IN (
            WITH RECURSIVE desc_ids(id) AS (
                SELECT id FROM projects WHERE parent_id = NEW.id
                UNION ALL
                SELECT p.id FROM projects p JOIN desc_ids d ON p.parent_id = d.id
            )
            SELECT id FROM desc_ids
       );
END;

-- 循環禁止（INSERT）: 新規IDに子孫は存在し得ないため自己参照のみ検査
-- （自己参照は account トリガーでも「親が存在しない」として偶発的に弾かれるが、
--  エラー理由が誤解を招くため専用トリガーで正しい理由を返す）
CREATE TRIGGER trg_projects_no_cycle_insert
BEFORE INSERT ON projects
WHEN NEW.parent_id IS NOT NULL AND NEW.parent_id = NEW.id
BEGIN
    SELECT RAISE(ABORT, 'project hierarchy cycle');
END;

-- 同一アカウント制約: 親が存在し、かつ同じ account_id であること
CREATE TRIGGER trg_projects_parent_account_insert
BEFORE INSERT ON projects
WHEN NEW.parent_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'parent project not found in same account')
    WHERE NOT EXISTS (
        SELECT 1 FROM projects pp
        WHERE pp.id = NEW.parent_id AND pp.account_id = NEW.account_id
    );
END;

CREATE TRIGGER trg_projects_parent_account_update
BEFORE UPDATE OF parent_id ON projects
WHEN NEW.parent_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'parent project not found in same account')
    WHERE NOT EXISTS (
        SELECT 1 FROM projects pp
        WHERE pp.id = NEW.parent_id AND pp.account_id = NEW.account_id
    );
END;

-- account_id の不変性をDBで強制する（宣言だけでは迂回経路で異アカウント親子が
-- 成立することが実機検証で確認されたため）
CREATE TRIGGER trg_projects_account_immutable
BEFORE UPDATE OF account_id ON projects
WHEN NEW.account_id != OLD.account_id
BEGIN
    SELECT RAISE(ABORT, 'project account_id is immutable');
END;
```

### 4.3 correction_log の再構築（既存バグの根治）

現状のバグ: `from_project` が `ON DELETE SET NULL` のため、マージで source を削除した瞬間に「source→target」の訂正ログが「未分類→target」という**誤った few-shot 学習例に変質**する。`to_project` は `ON DELETE CASCADE` のため訂正先案件の削除でログ自体が消える。階層化で構造再編（マージ・削除）が増えるため、v19 で根治する。

SQLite は FK の変更ができないためテーブル再構築（CREATE→INSERT SELECT→シーケンス引き継ぎ→DROP→RENAME）:

```sql
CREATE TABLE correction_log_new (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    mail_id      TEXT NOT NULL REFERENCES mails(id) ON DELETE CASCADE,
    from_project TEXT REFERENCES projects(id) ON DELETE SET NULL,
    from_path    TEXT,           -- 訂正時点のパススナップショット（例: 「ツアー > 埼玉 > 照明」）
    to_project   TEXT REFERENCES projects(id) ON DELETE SET NULL,  -- CASCADE→SET NULL に変更
    to_path      TEXT NOT NULL,  -- 訂正時点のパススナップショット
    corrected_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO correction_log_new (id, mail_id, from_project, from_path, to_project, to_path, corrected_at)
SELECT cl.id, cl.mail_id, cl.from_project, fp.name, cl.to_project, COALESCE(tp.name, ''), cl.corrected_at
FROM correction_log cl
LEFT JOIN projects fp ON cl.from_project = fp.id
LEFT JOIN projects tp ON cl.to_project = tp.id;
-- AUTOINCREMENT の高水位（発行済みROWIDの不再利用保証）を引き継ぐ。
-- 直前の明示ID付き INSERT SELECT の時点で correction_log_new の行は
-- sqlite_sequence に自動作成されているため、単純な UPDATE で足りる
-- （sqlite_sequence の name 列には UNIQUE 制約が無いため ON CONFLICT(name) は
--  構文エラーになる。UPDATE 方式は空テーブル・0件コピーの全エッジケースで
--  実機検証済み）
UPDATE sqlite_sequence
SET seq = (SELECT seq FROM sqlite_sequence WHERE name = 'correction_log')
WHERE name = 'correction_log_new'
  AND (SELECT seq FROM sqlite_sequence WHERE name = 'correction_log') > seq;
DROP TABLE correction_log;
ALTER TABLE correction_log_new RENAME TO correction_log;
```

- 既存行の from_path/to_path は移行時点の名前で埋める（現状フラットなので名前=パス）。from_project が既に NULL の行は from_path も NULL（=未分類からの移動として正しい）
- sqlite_sequence の RENAME 追従は SQLite が自動で行う

**消費コードの追従（これを怠ると根治が骨抜きになる）**:

- `db/assignments.rs::insert_correction` — シグネチャを `(conn, mail_id, from_project: Option<&str>, from_path: Option<&str>, to_project: &str, to_path: &str)` に変更。呼び出し元（`reassign_with_correction`、`approve_classification_in_tx` 経路）は `project_path_string` でパスを解決して渡す
- `db/assignments.rs::get_recent_corrections` — **全面書き換え必須**。現行は `JOIN projects pt ON cl.to_project = pt.id`（INNER JOIN）で現在の案件名をその場解決しており、v19 後は「参照先が削除された訂正ログ」を結果から除外してしまう（=根治したはずのケースを few-shot から消し続ける）。projects への JOIN を廃止し `from_path`/`to_path` を直接 SELECT する
- `models/classifier.rs::CorrectionEntry` — フィールドを `from_path: Option<String>` / `to_path: String` に変更。プロンプト生成（`classifier/prompt.rs` の訂正履歴行）もパス表記に追従

### 4.4 マイグレーション前提の検査

- `run_migrations` 冒頭で `PRAGMA foreign_keys` が 1 であることを検査し、無効なら AppError で中断（FK 未強制の接続でスキーマだけ進む事故を防ぐ）

### 4.5 db/projects.rs 追加・変更関数

| 関数 | 契約 |
|---|---|
| `subtree_ids(conn, id) -> Vec<String>` | 再帰CTEで自分+全子孫。**深い順（depth DESC）で返す**（削除の葉先行に必要）。深さ上限なし |
| `ancestor_path(conn, id) -> Vec<Project>` | ルート→自ノード。深さ上限なし |
| `project_path_string(conn, id) -> String` | 「ツアー > 会場 > 音響」形式（パンくず・LLM・スナップショット用） |
| `set_parent(conn, id, new_parent: Option<&str>)` | 存在・同一アカウント・循環・アーカイブ済み親を検証して付け替え |
| `list_projects` | parent_id 込みのフラット配列（ツリー構築はフロント）。アーカイブ除外は現行通り |
| `delete_project` | subtree_ids（深い順）で列挙し**葉先行**で1トランザクション明示削除 |
| `archive_project` | サブツリー一括を1トランザクションで `is_archived = TRUE` |
| `merge_projects` | 検証強化（§5） |
| `build_effective_context(conn, id)` | §7 参照 |

## 5. 操作セマンティクス

すべての階層変更操作は **1トランザクション**で行い、ADR-0004 に沿って **UseCase 化して dispatch バス経由**にする。現状の案件系コマンドは DB 直呼びで未移行のため、この移行自体を本件のスコープに含める（一部だけ dispatch に乗せると同種操作の認可・監査経路が割れる）。`update_project` も、名前変更がパス表示・LLM提示・パンくず・スナップショットに波及する階層上の書き込みなので対象。

| UseCase | Risk | 理由 |
|---|---|---|
| create_project（parent_id 付き） | Reversible | 削除で戻せる |
| update_project | Reversible | 再更新で戻せる |
| set_project_parent（新規） | Reversible | 付け替え直しで戻せる |
| archive_project | **Sensitive** | 復元 UseCase が v1 スコープ外のため実質不可逆 |
| delete_project | **Sensitive** | サブツリー+付随データ（ディレクトリ紐付け・コンテキスト・ファイルインベントリ・クラウドルール）を削除 |
| merge_projects | **Sensitive** | source 削除を含む |

| 操作 | 挙動 |
|---|---|
| 作成 | `create_project` に任意の `parent_id`。検証: 親の存在・同一アカウント・**アーカイブ済み親の下は不可**。UI: コンテキストメニュー「＋ 子案件を作成」 |
| 付け替え | `set_project_parent(project_id, new_parent_id: Option)`。検証: 存在・同一アカウント・循環（自分/子孫不可）・**アーカイブ済みノードへは不可**。UI: 「親を変更...」→ツリーピッカー（自分と子孫は disabled） |
| 削除 | subtree_ids（深い順）で列挙→**葉先行**で明示削除。確認ダイアログ: 配下案件数・影響メール数+「配下のメールは未分類に戻ります。同じスレッドに他案件のメールがある場合、AI が再分類することがあります」（`auto_follow_threads` の既存挙動を仕様として許容し、文言と実挙動を一致させる） |
| アーカイブ | サブツリー一括 `is_archived = TRUE`（親だけ消えて子が宙に浮く状態を作らない） |
| マージ | `merge_projects(source, target)`。検証: `source != target`・同一アカウント・target が source の子孫なら拒否（「統合先が統合元の配下にあります」）・**source / target とも `is_archived = FALSE`**（アーカイブ済み target へのマージは「非表示の親を持つ宙に浮いた子」を作ることが実機検証で確認されたため）。処理順: (1) source 直属メールを target へ reassign（訂正ログはパススナップショット付き）→ (2) source の子を target の子へ reparent → (3) source を削除 |

### 5.1 集約表示

- ノード N 選択で `subtree_ids(N)` 配下の全メールをスレッド表示（`get_threads_by_project` の意味変更——本設計の破壊的変更の中心）
- **戻り値の型**: `Mail` は変更しない（検索の `SearchResult` ラッパーと同じ流儀）。`Thread` に `projects: Vec<ThreadProjectRef { project_id, display_path }>` を追加し、メンバーメールの**直接所属案件の集合**を返す。`display_path` は**選択ノードからの相対パス**（階層内では同名案件が共存し得るため単一 name にしない）。全メールが選択ノード直属なら空配列（UI はラベル省略）
- **データフロー**: `threading.rs::build_threads` は `Mail` のみから組み立てるため（`Mail` は案件情報を持たない）、`get_threads_by_project` 側で `mail_id → project_id` の対応表を assignments から取得し、`build_threads` の**外側で** `Thread.projects` を付与する
- **未読バッジ**: `get_unread_counts` は現行通り「ノード直接所属の未読数」を返し、**集約はフロントのツリー構築時にボトムアップ加算**（ノード数ぶんの再帰CTE実行を避ける）
- **アーカイブ済み子孫の扱い**: 集約表示は**アーカイブ済みサブツリーのメールを含めない**。アーカイブ済み案件のメールは表示されないという従来挙動と一貫させ、サイドバーに存在しないノードへのチップ表示という不整合を避ける（実装レビューでの決定事項）

### 5.2 フロントエンドの状態整合契約

- 構造変更操作（作成/付け替え/削除/アーカイブ/マージ）の成功後は**案件一覧を再取得**し、ローカルの1件 add/remove に頼らない
- 消えたノードの project_id 別キャッシュ（directories / contexts / scanningProjects）を掃除する
- `selectedProjectId` が消滅・アーカイブされたサブツリーに含まれる場合は選択解除
- **非同期レスポンスの無効化**: 進行中の `fetchDirectory` / `fetchProjectContext` 等がキャッシュ掃除後に到着して書き戻す競合を防ぐため、**レスポンス反映前に対象 project_id が現在の案件一覧に存在するか確認し、存在しなければ破棄**（現行ストアは無条件で書き戻す）

## 6. AI分類

- `ProjectSummary` に `path`（「AKB48アリーナツアー > 埼玉スーパーアリーナ > 音響」形式）を追加し、プロンプトの案件リストをパス付きで列挙（ツリー構造はパス表記で伝える）
- システムプロンプトに追加: **「確信できる最も深いノードに割り当てよ。子のどれか確信が持てない場合は親を選べ」**
- `create` アクションに任意の `parent_project_id` を追加（既存案件配下への子案件提案）。検証: 存在・同一アカウント（既存 `is_assignable_project` と同型のハルシネーション対策）。**不正な parent はルート作成として扱う**（create は元々ユーザー承認制で、承認ダイアログに作成位置を表示・変更可能なため安全）
- 確信度ゲートは変更なし（`CONFIDENCE_UNCERTAIN`=0.4 の単一しきい値で assign を永続化。0.7 はUIバッジ配色用）
- 訂正 few-shot は correction_log の**パススナップショット**（from_path/to_path）を使う（§4.3）。「埼玉 > 音響 から 埼玉 > 照明 へ移動」という粒度の学習例になる
- クラウドLLM選択時の送信内容は従来と同じ境界（ADR-0002）。パスは案件名の合成であり新たな送信カテゴリを増やさない

## 7. 設定の加算的継承

- 新関数 `build_effective_context(conn, project_id)`: `ancestor_path` に沿ってルート→自ノードのディレクトリ+コンテキストを合算し、各項目に「定義元ノードID」を付けて返す
- クラウド送信可否は定義元ノードに紐づくルールで評価（`project_cloud_rules` はディレクトリ経由で定義元に付いているため**スキーマ変更不要**。継承されるのは「情報+その情報自身のルール」のパッケージ。ルール同士の合成は発生しない）
- 分類プロンプトでは各ノードは**自分の分のコンテキストのみ**表示（祖先も自分のエントリとして列挙されるため重複させない）
- UI: コンテキスト設定画面に継承分を「継承: <定義元ノード名>」ラベル付き読み取り専用で表示、自ノード分は編集可

## 8. サブツリー限定検索

- 検索バックエンドに `project_id: Option<String>` スコープ引数を追加。指定時は `subtree_ids` で展開し `mail_project_assignments.project_id IN (...)` でフィルタ。**未分類メールはスコープ指定時は対象外**（案件内検索の自然な意味）
- 適用対象: FTS（`db/search.rs::search_mails`）と**セマンティック検索（`db/vec_search.rs::search_mails_semantic`、mainマージ済み）の両方**に同じシグネチャで追加
- **セマンティック検索の取りこぼし対策**: 既存実装は「KNN→後段で account_id フィルタ」方式で、上位k件を対象外が占有すると取りこぼす既知の制限がある。サブツリーは母集団がさらに狭いため悪化する。**v1 の方針: スコープ指定時は k を常に上限（KNN_MAX=200）まで拡大**し、それでも小さい案件で取りこぼしが残ることを既知の制限としてコードコメントと設計書に明記して許容する（チャンク側テーブルへの project 列の非正規化は行わない。割当変更のたびに vec 索引を書き換える同期義務が生まれ、規模に対して過剰なため）
- UI: 案件選択中に検索欄へ「この案件内で検索」トグル。デフォルトOFF（アカウント全体）

## 9. UI

- `projectStore`: フラット配列から `parent_id` でツリー構築。展開状態は `Set<projectId>` で保持し localStorage 永続化
- `ProjectTree`: 再帰レンダリング（インデント+シェブロン）。未読バッジはボトムアップ加算の集約値
- スレッド一覧ヘッダに選択ノードのパンくず（パス）表示
- コンテキストメニュー追加: 「＋ 子案件を作成」「親を変更...」
- 親ノード閲覧時のスレッド行に `Thread.projects` の相対パスチップ表示
- メール→案件 D&D は任意ノードをターゲットにできる（既存機構のまま）

## 10. 破壊的変更の一覧

- `get_threads_by_project` の意味変更（直接所属のみ→サブツリー集約）
- `Thread` 型への `projects` フィールド追加（Rust/TS 両方）
- `Project` 型への `parent_id` 追加（Rust/TS 両方）
- `correction_log` スキーマ変更（from_path/to_path 追加、to_project の FK 方針変更）と消費コード（insert_correction / get_recent_corrections / CorrectionEntry）の変更
- 分類プロンプト形式の変更（パス付き列挙・create with parent）
- 案件系コマンドの dispatch バス移行（コマンド名・戻り値は維持）

## 11. テスト（TDD）

### Rust
- migration v19: `PRAGMA foreign_key_list(projects)`・`foreign_key_check`・correction_log 再構築のデータ保全・**sqlite_sequence 高水位の引き継ぎ**（旧max ID 削除後の再利用が起きないこと）
- トリガー: 循環（INSERT自己参照/UPDATE自分/UPDATE子孫）・異アカウント親・account_id 更新拒否
- subtree/ancestor 関数: 深い順契約・パス文字列
- **深さ1001超のサブツリーの葉先行削除の回帰テスト**（再帰CASCADE上限の回避を保証）
- マージ: 正常系・self 拒否・子孫 target 拒否・アーカイブ済み source/target 拒否・異アカウント拒否・子の reparent・スナップショット記録
- get_recent_corrections: 参照先案件の削除後もパススナップショットで few-shot が返ること
- 分類: パス付きプロンプト生成・create with parent の検証とルートフォールバック
- 検索: FTS/セマンティック両方のサブツリースコープ・未分類除外
- 集約: get_threads_by_project のサブツリー展開・Thread.projects の相対パス

### React
- ツリー描画・展開/折りたたみ・集約未読バッジ
- 「親を変更」ダイアログ（自分・子孫 disabled）
- 検索スコープトグル
- 状態整合: 削除後のキャッシュ掃除・選択解除・遅延レスポンス破棄

## 12. 段階的実装順（Stacked PR）

1. **PR①: DB基盤** — migration v19（parent_id・トリガー・correction_log 再構築+シーケンス引き継ぎ）・subtree/ancestor/path 関数・delete/archive のサブツリー化・葉先行削除・深さ回帰テスト
2. **PR②: 操作系** — set_parent・マージ検証強化・insert_correction/get_recent_corrections/CorrectionEntry のパススナップショット化・案件系 UseCase 化+dispatch 移行（Risk 定義含む）
3. **PR③: 分類器** — ProjectSummary.path・プロンプト改訂・create with parent・few-shot のパス化
4. **PR④: 検索** — FTS/セマンティックのサブツリースコープ
5. **PR⑤: UI** — ツリー描画・集約表示・パンくず・操作メニュー・検索トグル・状態整合契約

依存: ②〜⑤は①に依存。③④は②と並行可能。⑤は②〜④の後。

## 13. 既知の制限

- セマンティック検索のサブツリースコープは KNN 後フィルタのため、小さい案件では上位k件からの取りこぼしが残る（§8）
- 深さ1000超の階層は FK の防御的 CASCADE が機能しない（正常経路の明示削除は動作する）。実用上到達しない深さであり許容
- アーカイブに復元操作が無いため、サブツリーアーカイブは実質不可逆（Risk=Sensitive で保護）
