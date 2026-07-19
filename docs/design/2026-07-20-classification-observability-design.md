# AI分類の観測可能性（Classification Observability） 設計書

- 作成日: 2026-07-20
- ステータス: 設計中
- 関連: `docs/design/2026-04-13-phase2-ai-classification-design.md`（確信度ゲート）、`docs/adr/0002-cloud-llm-data-boundary.md`（クラウド送信境界）

## 1. 背景と目的

実データを調べたところ、AI分類の確信度がまったくキャリブレーションされていないことが判明した。

LLM由来の割り当て（`assigned_by='ai'` かつ `confidence` 非NULL）244件の分布は以下の通り。

| 確信度 | 件数 |
|---|---|
| 0.9 | 3 |
| 0.95 | 72 |
| 0.98 | 98 |
| 0.99 | 24 |
| 1.0 | 47 |

**0.9未満が1件も存在しない**。さらに、ユーザーが別案件へ訂正した28件の「訂正前の確信度」を見ると、過信が定量的に確認できる。

| 訂正前の確信度 | 訂正された件数 |
|---|---|
| 0.9 | 2 |
| 0.95 | 15 |
| 0.98 | 2 |
| 1.0 | 8 |

**「1.0（完全に確実）」と申告して8件外している**。確信度と正答率に相関がない。

結果として、設計書 `2026-04-13-phase2-ai-classification-design.md:403-406` が定めた確信度ゲート（`CONFIDENCE_AUTO_ASSIGN = 0.7` / `CONFIDENCE_UNCERTAIN = 0.4`）は、全件が0.9以上であるため**一度も作動していない**。安全弁が死んでいる。

### なぜ観測が先か

確信度の偏りを直す手段はプロンプトのキャリブレーション指示だが、**LLMのキャリブレーションがプロンプトで改善する保証はない**。特に本アプリが既定とする小〜中規模モデル（Gemini Flash / Claude Haiku / llama3.1:8b）は自己申告確信度が構造的に粗い。

したがって「直す」前に「測れる」状態を作る。現状では以下が観測できない。

- **確信度0.4未満で破棄されたassign** — `classifier/service.rs:383-389` がDBに書かずUnclassifiedへ正規化するため痕跡が残らない
- **create / unclassified を選んだときの確信度** — `mail_project_assignments` に行が作られないため記録されない

つまり `mail_project_assignments.confidence` は「AIが出した確信度の分布」ではなく、**「assignを選び、かつ0.4以上だった場合」という二重に切り取られた条件付き分布**である。これを生の分布と誤読すると、プロンプト修正の効果を測り違える。

## 2. スコープ

### やること
- 新テーブル `classification_log` に、AIの**全判断を記録**する（assign / create / unclassified の別を問わず、破棄されたものも含む）
- 記録項目は「いつ・どのメールを・何と判断し・確信度はいくつで・実際に永続化されたか」
- 既存の分類フローから漏れなく記録する（`classifier::service::apply_result` の1箇所に集約）

### やらないこと（YAGNI）
- 統計を返すTauri commandやUI（本設計では記録のみ。SQLで確認する）
- プロンプトのキャリブレーション修正（本設計の後、効果測定つきで別途行う）
- 確信度閾値の変更（測ってから判断する）
- 既存データの遡及生成（過去の判断は復元不能。記録は本実装以降のみ）
- ログの自動削除・ローテーション（まず溜める。肥大化が問題になってから対処）

## 3. 設計

### 3.1 テーブル

migration v22 で追加する（現在 v21）。

```sql
CREATE TABLE classification_log (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    mail_id       TEXT NOT NULL REFERENCES mails(id) ON DELETE CASCADE,
    account_id    TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- AI が選んだ行動: 'assign' | 'create' | 'unclassified'
    action        TEXT NOT NULL CHECK(action IN ('assign', 'create', 'unclassified')),
    -- assign のとき提案先の案件。case は残すが案件削除で参照は切れる
    project_id    TEXT REFERENCES projects(id) ON DELETE SET NULL,
    -- 提案時点の案件パスのスナップショット（案件が消えても意味を保つ）
    project_path  TEXT,
    -- create のとき提案された案件名
    proposed_name TEXT,
    confidence    REAL NOT NULL,
    -- 確信度ゲートを通過して実際に mail_project_assignments へ書かれたか
    persisted     INTEGER NOT NULL CHECK(persisted IN (0, 1)),
    -- 判断に使ったモデル（プロバイダ:モデル名）。キャリブレーション比較の軸
    model         TEXT,
    classified_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_classification_log_mail ON classification_log(mail_id);
CREATE INDEX idx_classification_log_at ON classification_log(classified_at);
```

### 3.2 記録する箇所

`classifier::service::apply_result`（`service.rs:362-424`）が唯一の書き込み点になる。この関数はassign / create / unclassifiedの全分岐を持ち、確信度ゲートもここにあるため、記録の抜けが構造的に起きない。

同関数はすでに `mail_project_assignments` への書き込みを行っているので、**同一トランザクション内**でログも書く。分類結果とログが食い違う状態を作らない。

### 3.3 `reason` を記録しない理由

`ClassifyResult` にはLLMが返す `reason`（判断理由の自由記述）がある。有用に見えるが記録しない。

`reason` はメール本文の要約や引用を含みうる。`classification_log` は分析目的で長期保存され、将来エクスポートやサポート用途で外部に出る可能性がある。`docs/adr/0002-cloud-llm-data-boundary.md` が定める「本文はLLMへ送るが、送った内容を新たな保存先に増やさない」という原則に照らし、**本文由来のテキストを新テーブルに複製しない**。

確信度のキャリブレーション分析に `reason` は不要であり、必要性が生じた時点で改めて設計する。

### 3.4 `model` を記録する理由

プロバイダ・モデルは設定でいつでも変更できる（`settings` テーブル）。モデルを変えれば確信度の性質も変わるため、モデル名なしのログは時系列で混ざって解釈できなくなる。

`llm_provider:model_name` 形式の文字列で保存する（例 `gemini_vertex:gemini-3.5-flash`）。APIキー等の秘密情報は含めない。

### 3.5 計測できるようになること

本テーブル単体、および `correction_log` との突合で以下が算出可能になる。

- **生の確信度分布**（破棄されたものを含む）— キャリブレーション改善の主指標
- **確信度帯ごとの実測正答率** — 「0.95と言った群は実際に何%正しかったか」
- **action の選択比率** — unclassified が過度に抑制されていないか
- **モデル別・時系列の比較** — プロンプト修正やモデル変更の前後で改善したか

「正答」の定義は暫定的に **「訂正されていないこと」** とする。ユーザーが未確認のまま放置している分類は正答として数えられてしまうため、これは**正答率の上限**である。⚠承認フロー（PR #205）が使われ始めれば、明示的な承認と未確認を区別できるようになる。

## 4. 検証方法

実装後、以下のSQLで記録を確認する。

```sql
-- 生の確信度分布（破棄分を含む）
SELECT action, persisted, ROUND(confidence, 2) c, COUNT(*)
FROM classification_log GROUP BY action, persisted, c ORDER BY action, c;

-- 確信度帯ごとの訂正率
SELECT ROUND(l.confidence, 2) c,
       COUNT(*) total,
       SUM(CASE WHEN cl.mail_id IS NOT NULL THEN 1 ELSE 0 END) corrected
FROM classification_log l
LEFT JOIN correction_log cl
       ON cl.mail_id = l.mail_id AND cl.from_project IS NOT NULL
WHERE l.action = 'assign' AND l.persisted = 1
GROUP BY c ORDER BY c;
```

## 5. 影響

- **書き込み量**: 分類1件につき1行。既存の分類フローに1 INSERTが増える。バッチ分類の性能への影響は軽微と見込むが、実測する
- **既存挙動**: 分類のロジック・閾値・UIは一切変更しない。記録の追加のみ
- **プライバシー**: 本文由来のテキストは保存しない（3.3参照）。件名も保存しない
