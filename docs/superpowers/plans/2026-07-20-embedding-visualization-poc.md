# 埋め込み空間の可視化 PoC 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `vec_chunks` に蓄積済みの bge-m3 埋め込みを DB から直読みし、PCA / t-SNE / UMAP で 2 次元に落として案件ごとに色分けした散布図 PNG を出力する Python スクリプトを作る。

**Architecture:** アプリ本体（Tauri / Rust / React）には一切手を入れない。`scripts/` 配下の独立した Python スクリプトとして実装する。埋め込みは生成済みなので DB を読むだけでよく、Ollama の再実行は不要。スクリプトは「DB 読み出し」「集約」「次元削減」「描画」の 4 つの純粋関数に分け、それぞれ単体テスト可能にする。

**Tech Stack:** Python 3.14（mise 管理）、numpy、scikit-learn、matplotlib、sqlite-vec、umap-learn（任意）

## このプランの位置づけ

設計書 `docs/design/2026-07-20-embedding-visualization-design.md` は二段構えで、本プランは**第 1 段階（PoC）のみ**を対象とする。

第 2 段階（Rust PCA + Tauri 独立ウィンドウ + Canvas）は、**PoC の較正結果を確認してから別プランとして書く**。PCA でどこまで見えるかによってアプリ側の設計が変わりうる（PCA で全く分離が見えない場合、設計書 §4.1 の代替案 C を再検討する）ため、今の段階で計画を書いても無駄になる可能性が高い。

## Global Constraints

- **アプリ本体のコード（`src-tauri/`, `src/`）を一切変更しない。** 追加は `scripts/` 配下と `.gitignore` / `mise.toml` のみ
- **DB は必ず読み取り専用で開く。** 実 DB は 896MB の本番データであり、書き込み事故は許容できない。接続は `sqlite3.connect(f"file:{path}?mode=ro", uri=True)` 形式のみ使う
- **埋め込みベクトルは 1024 次元 float32。** `vec_chunks.embedding` は `zerocopy::IntoBytes` による生バイト列なので `np.frombuffer(blob, dtype=np.float32)` で復元する（`src-tauri/src/db/chunks.rs:82`）
- **`vec0` 仮想テーブルの読み取りには sqlite-vec 拡張のロードが必須**
- 既定の DB パス: macOS は `~/Library/Application Support/Pigeon/pigeon.db`、Linux は `~/.local/share/Pigeon/pigeon.db`、Windows は `%APPDATA%\Pigeon\pigeon.db`（`src-tauri/src/lib.rs:40-46` の `dirs::data_dir()` に対応）
- コミットメッセージは Conventional Commits 形式、scope は `scripts` を使う

## 実データの実態（2026-07-20 時点、計画の前提）

実装前に把握しておくべき数字。**これが描画設計を左右する。**

| 項目 | 件数 |
|---|---|
| メール | 11,531 |
| チャンク | 33,241（全件埋め込み済み） |
| 案件 | 14 |
| 案件割り当て | **863（メール全体の 7.5%）** |

案件別の内訳は上位に極端に偏る: Googleの通知 298 / 勤怠管理 292 / Atlassian 192 — **上位 3 件で割り当て済みの 9 割**。残り 11 案件は 25 件以下、うち 4 件は 1 件のみ。

ここから導かれる 2 つの設計判断:

1. **未分類メール 10,668 件のグレー点が、色付き 863 点を完全に覆い隠す。** 素朴に全件描くと何も読めない。`--assigned-only` フィルタ（既定 ON）を Task 5 で用意する
2. **上位 3 案件はいずれも送信者が固定された通知メール。** 分離して見えても「bge-m3 が意味を捉えた」証拠としては弱い（件名の定型文が効いているだけの可能性がある）。この解釈上の注意は Task 8 の README に明記する

## File Structure

| ファイル | 責務 |
|---|---|
| `scripts/visualize_embeddings.py` | CLI エントリポイント。引数解析と 4 段階の呼び出しのみ |
| `scripts/embedding_viz/db.py` | DB 接続と読み出し。sqlite-vec ロード、JOIN クエリ、blob 復元 |
| `scripts/embedding_viz/aggregate.py` | チャンク → メール単位の centroid 集約 |
| `scripts/embedding_viz/reduce.py` | 次元削減（PCA / t-SNE / UMAP の切替） |
| `scripts/embedding_viz/plot.py` | matplotlib による散布図描画と PNG 保存 |
| `scripts/tests/test_*.py` | 各モジュールの単体テスト |
| `scripts/requirements.txt` | 依存パッケージ |
| `scripts/README.md` | 実行手順・手法の使い分け・結果の解釈上の注意 |

**なぜ 1 ファイルにしないか:** 設計書は「PoC はテスト対象外（使い捨ての調査ツール）」としているが、これは*スクリプト全体を通しで動かす統合テスト*を書かないという意味である。集約・次元削減・座標変換には**間違えても絵が出てしまう**種類のロジック（centroid の軸、PCA の中心化忘れ）が含まれ、間違いに気づけない。ここだけは単体テストで固める。DB 読み出しと描画は目視で足りる。

---

### Task 1: Python 環境と依存のセットアップ

**Files:**
- Modify: `mise.toml`
- Create: `scripts/requirements.txt`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: なし
- Produces: `scripts/` 配下で `python3` と numpy / scikit-learn / matplotlib / sqlite-vec が使える環境

- [ ] **Step 1: mise に python を追加**

`mise.toml` を以下に書き換える（既存の 3 行に 1 行足すだけ）:

```toml
[tools]
node = "22"
pnpm = "latest"
rust = "latest"
python = "3.14"
```

- [ ] **Step 2: requirements.txt を作る**

`scripts/requirements.txt`:

```
numpy>=2.0
scikit-learn>=1.5
matplotlib>=3.9
sqlite-vec>=0.1.6
# UMAP は任意。--method umap を使う場合のみ必要（インストールが重いため既定では入れない）
# umap-learn>=0.5
```

- [ ] **Step 3: .gitignore に Python 系と出力 PNG を追加**

`.gitignore` の末尾に追記する:

```gitignore

# Python（scripts/ 配下の可視化ツール）
__pycache__/
*.py[cod]
.venv/
scripts/.venv/
scripts/out/
```

`scripts/out/` は散布図 PNG の既定出力先。生成物なのでコミットしない。

- [ ] **Step 4: venv を作って依存をインストール**

```bash
cd scripts && python3 -m venv .venv && .venv/bin/pip install -q -r requirements.txt && .venv/bin/python -c "import numpy, sklearn, matplotlib, sqlite_vec; print('deps OK')"
```

Expected: `deps OK`

- [ ] **Step 5: sqlite-vec が実際にロードできることを確認**

これが PoC 最大の失敗要因なので、他を書く前に潰しておく:

```bash
cd scripts && .venv/bin/python -c "
import sqlite3, sqlite_vec
c = sqlite3.connect(':memory:')
c.enable_load_extension(True)
sqlite_vec.load(c)
c.enable_load_extension(False)
print('sqlite-vec OK:', c.execute('select vec_version()').fetchone()[0])
"
```

Expected: `sqlite-vec OK: v0.1.x`

失敗した場合は次を確認する: (a) mise の Python が拡張ロード対応でビルドされているか（`hasattr(conn, 'enable_load_extension')`）、(b) システムの Python を使っていないか（macOS 標準の Python は拡張ロードが無効化されている）

- [ ] **Step 6: コミット**

```bash
git add mise.toml scripts/requirements.txt .gitignore
git commit -m "chore(scripts): 埋め込み可視化PoC用のPython環境を追加"
```

---

### Task 2: DB 読み出し（sqlite-vec ロードとベクトル復元）

**Files:**
- Create: `scripts/embedding_viz/__init__.py`（空ファイル）
- Create: `scripts/embedding_viz/db.py`
- Create: `scripts/tests/__init__.py`（空ファイル）
- Test: `scripts/tests/test_db.py`

**Interfaces:**
- Consumes: Task 1 の依存
- Produces:
  - `default_db_path() -> pathlib.Path`
  - `connect(db_path: pathlib.Path) -> sqlite3.Connection` — 読み取り専用、sqlite-vec ロード済み
  - `ChunkRow` — `NamedTuple(chunk_id: int, mail_id: str, subject: str, project_id: str | None, project_name: str | None, vector: np.ndarray)`
  - `load_chunks(conn, limit: int | None, assigned_only: bool) -> list[ChunkRow]`

- [ ] **Step 1: 失敗するテストを書く**

`scripts/tests/test_db.py`:

```python
import sqlite3
import numpy as np
import pytest
from embedding_viz.db import decode_vector, build_query


def test_decode_vector_roundtrips_float32():
    original = np.array([0.5, -1.25, 3.0], dtype=np.float32)
    blob = original.tobytes()
    assert np.array_equal(decode_vector(blob), original)


def test_decode_vector_rejects_wrong_size():
    # float32 は 4 バイト単位。5 バイトは壊れたデータ。
    with pytest.raises(ValueError):
        decode_vector(b"\x00" * 5)


def test_build_query_assigned_only_filters_null_project():
    sql, params = build_query(limit=None, assigned_only=True)
    assert "mpa.project_id IS NOT NULL" in sql
    assert params == []


def test_build_query_without_filter_keeps_unassigned():
    sql, params = build_query(limit=None, assigned_only=False)
    assert "mpa.project_id IS NOT NULL" not in sql


def test_build_query_applies_limit_as_parameter():
    sql, params = build_query(limit=500, assigned_only=True)
    assert "LIMIT ?" in sql
    assert params == [500]
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
cd scripts && .venv/bin/python -m pytest tests/test_db.py -v
```

Expected: FAIL — `ModuleNotFoundError: No module named 'embedding_viz'`

- [ ] **Step 3: 実装する**

`scripts/embedding_viz/__init__.py` は空ファイルを作る。

`scripts/embedding_viz/db.py`:

```python
"""Pigeon の SQLite DB から埋め込みベクトルとラベルを読み出す。

DB は必ず読み取り専用で開く。実 DB は本番データであり、書き込み事故は許容できない。
"""

from __future__ import annotations

import os
import sqlite3
import sys
from pathlib import Path
from typing import NamedTuple

import numpy as np
import sqlite_vec

# bge-m3 の次元数。vec_chunks の float[1024] に対応する（src-tauri/src/db/migrations.rs の v18）。
EMBEDDING_DIM = 1024


class ChunkRow(NamedTuple):
    chunk_id: int
    mail_id: str
    subject: str
    project_id: str | None
    project_name: str | None
    vector: np.ndarray


def default_db_path() -> Path:
    """src-tauri/src/lib.rs の dirs::data_dir().join("Pigeon") に対応する既定パス。"""
    if sys.platform == "darwin":
        base = Path.home() / "Library" / "Application Support"
    elif sys.platform == "win32":
        base = Path(os.environ.get("APPDATA", Path.home()))
    else:
        base = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local" / "share"))
    return base / "Pigeon" / "pigeon.db"


def decode_vector(blob: bytes) -> np.ndarray:
    """vec_chunks.embedding の生バイト列を float32 配列へ復元する。

    Rust 側は zerocopy::IntoBytes で &[f32] をそのまま BLOB 化している
    （src-tauri/src/db/chunks.rs:82）ので、frombuffer で読み直せる。
    """
    if len(blob) % 4 != 0:
        raise ValueError(f"float32 の倍数長ではありません: {len(blob)} バイト")
    return np.frombuffer(blob, dtype=np.float32)


def connect(db_path: Path) -> sqlite3.Connection:
    """読み取り専用で接続し、sqlite-vec 拡張をロードする。

    vec0 仮想テーブル（vec_chunks）を読むには拡張のロードが必須。
    """
    if not db_path.exists():
        raise FileNotFoundError(f"DB が見つかりません: {db_path}")
    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    try:
        conn.enable_load_extension(True)
        sqlite_vec.load(conn)
        conn.enable_load_extension(False)
    except AttributeError as exc:
        raise RuntimeError(
            "この Python は拡張ロードに対応していません。"
            "macOS 標準の Python ではなく mise / pyenv 等でビルドされた Python を使ってください。"
        ) from exc
    return conn


def build_query(limit: int | None, assigned_only: bool) -> tuple[str, list]:
    """チャンク取得クエリを組み立てる。

    JOIN 構造は src-tauri/src/db/vec_search.rs の search_mails_semantic を踏襲する。
    limit は必ずパラメータとして渡す（SQL 文字列に埋め込まない）。
    """
    where = "WHERE mpa.project_id IS NOT NULL" if assigned_only else ""
    limit_clause = "LIMIT ?" if limit is not None else ""
    params: list = [limit] if limit is not None else []
    sql = f"""
        SELECT v.chunk_id, c.mail_id, m.subject, mpa.project_id, p.name, v.embedding
        FROM vec_chunks v
        JOIN mail_chunks c ON c.id = v.chunk_id
        JOIN mails m ON m.id = c.mail_id
        LEFT JOIN mail_project_assignments mpa ON mpa.mail_id = m.id
        LEFT JOIN projects p ON p.id = mpa.project_id
        {where}
        ORDER BY c.mail_id, c.chunk_index
        {limit_clause}
    """
    return sql, params


def load_chunks(
    conn: sqlite3.Connection, limit: int | None, assigned_only: bool
) -> list[ChunkRow]:
    """チャンク単位でベクトルとラベルを読み出す。

    次元が想定と違う行はスキップする（モデル変更後の再埋め込み途中など）。
    """
    sql, params = build_query(limit, assigned_only)
    rows: list[ChunkRow] = []
    skipped = 0
    for chunk_id, mail_id, subject, project_id, project_name, blob in conn.execute(
        sql, params
    ):
        vector = decode_vector(blob)
        if vector.shape[0] != EMBEDDING_DIM:
            skipped += 1
            continue
        rows.append(
            ChunkRow(
                chunk_id=chunk_id,
                mail_id=mail_id,
                subject=subject or "(件名なし)",
                project_id=project_id,
                project_name=project_name,
                vector=vector,
            )
        )
    if skipped:
        print(f"警告: 次元が {EMBEDDING_DIM} でない行を {skipped} 件スキップしました")
    return rows
```

- [ ] **Step 4: テストが通ることを確認**

```bash
cd scripts && .venv/bin/python -m pytest tests/test_db.py -v
```

Expected: 5 passed

pytest が未インストールなら `.venv/bin/pip install -q pytest` を実行し、`requirements.txt` の末尾に `pytest>=8.0` を追記する。

- [ ] **Step 5: 実 DB に対して読めることを確認**

```bash
cd scripts && .venv/bin/python -c "
from embedding_viz.db import connect, default_db_path, load_chunks
conn = connect(default_db_path())
rows = load_chunks(conn, limit=50, assigned_only=True)
print('rows:', len(rows), '| dim:', rows[0].vector.shape, '| project:', rows[0].project_name)
"
```

Expected: `rows: 50 | dim: (1024,) | project: <案件名>`

- [ ] **Step 6: コミット**

```bash
git add scripts/embedding_viz/__init__.py scripts/embedding_viz/db.py scripts/tests/__init__.py scripts/tests/test_db.py scripts/requirements.txt
git commit -m "feat(scripts): 埋め込みベクトルのDB読み出しを追加"
```

---

### Task 3: メール単位への centroid 集約

**Files:**
- Create: `scripts/embedding_viz/aggregate.py`
- Test: `scripts/tests/test_aggregate.py`

**Interfaces:**
- Consumes: `ChunkRow`（Task 2）
- Produces:
  - `Point` — `NamedTuple(label_id: str, subject: str, project_id: str | None, project_name: str | None, vector: np.ndarray)`
  - `to_chunk_points(rows: list[ChunkRow]) -> list[Point]`
  - `to_mail_points(rows: list[ChunkRow]) -> list[Point]`

- [ ] **Step 1: 失敗するテストを書く**

`scripts/tests/test_aggregate.py`:

```python
import numpy as np
from embedding_viz.aggregate import to_chunk_points, to_mail_points
from embedding_viz.db import ChunkRow


def _row(chunk_id, mail_id, vector, project_id="p1", project_name="案件A"):
    return ChunkRow(
        chunk_id=chunk_id,
        mail_id=mail_id,
        subject=f"件名{mail_id}",
        project_id=project_id,
        project_name=project_name,
        vector=np.array(vector, dtype=np.float32),
    )


def test_chunk_points_keep_every_row():
    rows = [_row(1, "m1", [1.0, 0.0]), _row(2, "m1", [0.0, 1.0])]
    points = to_chunk_points(rows)
    assert len(points) == 2
    assert points[0].label_id == "1"


def test_mail_points_average_chunks_of_same_mail():
    rows = [_row(1, "m1", [1.0, 0.0]), _row(2, "m1", [0.0, 2.0])]
    points = to_mail_points(rows)
    assert len(points) == 1
    # centroid は要素ごとの平均。軸を取り違えると全体平均になって潰れる。
    assert np.allclose(points[0].vector, [0.5, 1.0])
    assert points[0].label_id == "m1"


def test_mail_points_separate_different_mails():
    rows = [_row(1, "m1", [1.0, 0.0]), _row(2, "m2", [0.0, 1.0])]
    points = to_mail_points(rows)
    assert len(points) == 2
    assert {p.label_id for p in points} == {"m1", "m2"}


def test_mail_points_preserve_label_from_first_chunk():
    rows = [
        _row(1, "m1", [1.0, 0.0], project_id="p9", project_name="案件Z"),
        _row(2, "m1", [0.0, 1.0], project_id="p9", project_name="案件Z"),
    ]
    points = to_mail_points(rows)
    assert points[0].project_name == "案件Z"
    assert points[0].subject == "件名m1"


def test_mail_points_handle_unassigned():
    rows = [_row(1, "m1", [1.0, 0.0], project_id=None, project_name=None)]
    points = to_mail_points(rows)
    assert points[0].project_id is None


def test_empty_input_returns_empty():
    assert to_mail_points([]) == []
    assert to_chunk_points([]) == []
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
cd scripts && .venv/bin/python -m pytest tests/test_aggregate.py -v
```

Expected: FAIL — `ModuleNotFoundError: No module named 'embedding_viz.aggregate'`

- [ ] **Step 3: 実装する**

`scripts/embedding_viz/aggregate.py`:

```python
"""チャンク単位のベクトルを、描画する点の単位へまとめる。"""

from __future__ import annotations

from typing import NamedTuple

import numpy as np

from .db import ChunkRow


class Point(NamedTuple):
    """散布図の 1 点。label_id はチャンク単位なら chunk_id、メール単位なら mail_id。"""

    label_id: str
    subject: str
    project_id: str | None
    project_name: str | None
    vector: np.ndarray


def to_chunk_points(rows: list[ChunkRow]) -> list[Point]:
    """チャンクをそのまま 1 点として扱う。"""
    return [
        Point(
            label_id=str(r.chunk_id),
            subject=r.subject,
            project_id=r.project_id,
            project_name=r.project_name,
            vector=r.vector,
        )
        for r in rows
    ]


def to_mail_points(rows: list[ChunkRow]) -> list[Point]:
    """同一メールのチャンクを centroid（要素ごとの平均）へまとめる。

    ラベル（件名・案件）は同一メール内で同じなので最初のチャンクのものを使う。
    """
    grouped: dict[str, list[ChunkRow]] = {}
    for row in rows:
        grouped.setdefault(row.mail_id, []).append(row)

    points: list[Point] = []
    for mail_id, group in grouped.items():
        stacked = np.stack([r.vector for r in group])
        # axis=0 で要素ごとの平均。axis を省くと全要素の平均（スカラー）になり点が潰れる。
        centroid = stacked.mean(axis=0)
        head = group[0]
        points.append(
            Point(
                label_id=mail_id,
                subject=head.subject,
                project_id=head.project_id,
                project_name=head.project_name,
                vector=centroid,
            )
        )
    return points
```

- [ ] **Step 4: テストが通ることを確認**

```bash
cd scripts && .venv/bin/python -m pytest tests/test_aggregate.py -v
```

Expected: 6 passed

- [ ] **Step 5: コミット**

```bash
git add scripts/embedding_viz/aggregate.py scripts/tests/test_aggregate.py
git commit -m "feat(scripts): チャンクをメール単位のcentroidへ集約する処理を追加"
```

---

### Task 4: 次元削減（PCA / t-SNE / UMAP）

**Files:**
- Create: `scripts/embedding_viz/reduce.py`
- Test: `scripts/tests/test_reduce.py`

**Interfaces:**
- Consumes: なし（numpy 配列のみを扱う純粋関数）
- Produces:
  - `reduce_dimensions(vectors: np.ndarray, method: str, seed: int = 42) -> np.ndarray` — `(N, 1024)` を受け `(N, 2)` を返す
  - `METHODS: tuple[str, ...]` — `("pca", "tsne", "umap")`

- [ ] **Step 1: 失敗するテストを書く**

`scripts/tests/test_reduce.py`:

```python
import numpy as np
import pytest
from embedding_viz.reduce import reduce_dimensions


def _two_cluster_data(n=30, dim=50, seed=0):
    """明確に離れた 2 クラスタを作る。次元削減後も分離が保たれるはず。"""
    rng = np.random.default_rng(seed)
    a = rng.normal(loc=0.0, scale=0.1, size=(n, dim))
    b = rng.normal(loc=10.0, scale=0.1, size=(n, dim))
    return np.vstack([a, b]).astype(np.float32)


def test_pca_returns_two_columns():
    coords = reduce_dimensions(_two_cluster_data(), method="pca")
    assert coords.shape == (60, 2)


def test_pca_preserves_cluster_separation():
    """PCA が正しく動けば、2 クラスタは第 1 主成分上で分離する。

    中心化を忘れると分離が崩れるので、それを検出できる。
    """
    data = _two_cluster_data()
    coords = reduce_dimensions(data, method="pca")
    first_half, second_half = coords[:30, 0], coords[30:, 0]
    # クラスタ内のばらつきより、クラスタ間の距離のほうが十分大きいこと
    gap = abs(first_half.mean() - second_half.mean())
    spread = max(first_half.std(), second_half.std())
    assert gap > spread * 5


def test_pca_is_deterministic():
    data = _two_cluster_data()
    assert np.allclose(
        reduce_dimensions(data, method="pca"), reduce_dimensions(data, method="pca")
    )


def test_tsne_returns_two_columns():
    coords = reduce_dimensions(_two_cluster_data(), method="tsne", seed=1)
    assert coords.shape == (60, 2)


def test_tsne_is_reproducible_with_same_seed():
    data = _two_cluster_data()
    a = reduce_dimensions(data, method="tsne", seed=7)
    b = reduce_dimensions(data, method="tsne", seed=7)
    assert np.allclose(a, b)


def test_unknown_method_raises():
    with pytest.raises(ValueError, match="不明な手法"):
        reduce_dimensions(_two_cluster_data(), method="magic")


def test_too_few_points_raises():
    """点が 2 未満だと主成分が定義できない。"""
    with pytest.raises(ValueError, match="2 点以上"):
        reduce_dimensions(np.zeros((1, 50), dtype=np.float32), method="pca")
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
cd scripts && .venv/bin/python -m pytest tests/test_reduce.py -v
```

Expected: FAIL — `ModuleNotFoundError: No module named 'embedding_viz.reduce'`

- [ ] **Step 3: 実装する**

`scripts/embedding_viz/reduce.py`:

```python
"""高次元の埋め込みを 2 次元へ落とす。

手法の使い分け:
  pca  — 線形・高速・決定的。大局的な構造を保つが、クラスタの分離は鮮明でない
  tsne — 局所構造を強調しクラスタが分離して見える。遅く、大域的な距離は信用できない
  umap — 中間。局所と大域のバランスが良いが umap-learn の追加インストールが必要
"""

from __future__ import annotations

import numpy as np
from sklearn.decomposition import PCA
from sklearn.manifold import TSNE

METHODS: tuple[str, ...] = ("pca", "tsne", "umap")


def reduce_dimensions(
    vectors: np.ndarray, method: str, seed: int = 42
) -> np.ndarray:
    """(N, D) の配列を (N, 2) へ落とす。"""
    if vectors.shape[0] < 2:
        raise ValueError(
            f"次元削減には 2 点以上が必要です（入力: {vectors.shape[0]} 点）"
        )

    # float64 へ寄せる。sklearn は内部で float64 を使うため、
    # 明示しておくと手法ごとの微妙な数値差を避けられる。
    data = np.asarray(vectors, dtype=np.float64)

    if method == "pca":
        # PCA は内部で中心化を行う。決定的なので seed は不要。
        return PCA(n_components=2, random_state=seed).fit_transform(data)

    if method == "tsne":
        # perplexity は点数より小さくないと sklearn がエラーを出す。
        # 既定 30 を、少数データでも動くよう安全側へ丸める。
        perplexity = min(30.0, max(5.0, (data.shape[0] - 1) / 3.0))
        return TSNE(
            n_components=2,
            random_state=seed,
            perplexity=perplexity,
            init="pca",
        ).fit_transform(data)

    if method == "umap":
        try:
            import umap
        except ImportError as exc:
            raise RuntimeError(
                "umap-learn がインストールされていません。"
                "`pip install umap-learn` を実行するか、--method pca / tsne を使ってください。"
            ) from exc
        n_neighbors = min(15, max(2, data.shape[0] - 1))
        return umap.UMAP(
            n_components=2, random_state=seed, n_neighbors=n_neighbors
        ).fit_transform(data)

    raise ValueError(f"不明な手法です: {method}（利用可能: {', '.join(METHODS)}）")
```

- [ ] **Step 4: テストが通ることを確認**

```bash
cd scripts && .venv/bin/python -m pytest tests/test_reduce.py -v
```

Expected: 7 passed（t-SNE のテストは数秒かかる）

- [ ] **Step 5: コミット**

```bash
git add scripts/embedding_viz/reduce.py scripts/tests/test_reduce.py
git commit -m "feat(scripts): PCA/t-SNE/UMAPによる次元削減を追加"
```

---

### Task 5: 散布図の描画

**Files:**
- Create: `scripts/embedding_viz/plot.py`
- Test: `scripts/tests/test_plot.py`

**Interfaces:**
- Consumes: `Point`（Task 3）
- Produces:
  - `group_by_project(points: list[Point]) -> dict[str, list[int]]` — 凡例ラベル → 点のインデックス列
  - `save_scatter(points, coords, out_path, title, max_legend=12) -> None`

- [ ] **Step 1: 失敗するテストを書く**

`scripts/tests/test_plot.py`:

```python
import numpy as np
from embedding_viz.aggregate import Point
from embedding_viz.plot import UNASSIGNED_LABEL, group_by_project, save_scatter


def _point(label_id, project_name):
    return Point(
        label_id=label_id,
        subject=f"件名{label_id}",
        project_id=None if project_name is None else f"p-{project_name}",
        project_name=project_name,
        vector=np.zeros(2, dtype=np.float32),
    )


def test_group_by_project_collects_indices():
    points = [_point("1", "A"), _point("2", "B"), _point("3", "A")]
    groups = group_by_project(points)
    assert groups["A"] == [0, 2]
    assert groups["B"] == [1]


def test_group_by_project_labels_unassigned():
    points = [_point("1", None)]
    groups = group_by_project(points)
    assert groups[UNASSIGNED_LABEL] == [0]


def test_group_by_project_orders_largest_first():
    """凡例が多すぎるときに大きい案件を優先して残すため、件数降順で並べる。"""
    points = [_point("1", "small"), _point("2", "big"), _point("3", "big")]
    assert list(group_by_project(points).keys())[0] == "big"


def test_save_scatter_writes_png(tmp_path):
    points = [_point("1", "A"), _point("2", "B")]
    coords = np.array([[0.0, 0.0], [1.0, 1.0]])
    out = tmp_path / "out.png"
    save_scatter(points, coords, out, title="test")
    assert out.exists()
    assert out.stat().st_size > 0
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
cd scripts && .venv/bin/python -m pytest tests/test_plot.py -v
```

Expected: FAIL — `ModuleNotFoundError: No module named 'embedding_viz.plot'`

- [ ] **Step 3: 実装する**

`scripts/embedding_viz/plot.py`:

```python
"""2 次元座標を案件ごとに色分けした散布図として描く。"""

from __future__ import annotations

from pathlib import Path

import matplotlib

# GUI バックエンドを避ける（ヘッドレス環境と CI で落ちないように）。
# pyplot の import より前に設定する必要がある。
matplotlib.use("Agg")

import matplotlib.pyplot as plt  # noqa: E402
import numpy as np  # noqa: E402

from .aggregate import Point  # noqa: E402

UNASSIGNED_LABEL = "未分類"

# 未分類は目立たせない。実データでは未分類が全体の 9 割を占めるため、
# 濃い色にすると案件の点を覆い隠してしまう。
UNASSIGNED_COLOR = "#cccccc"


def group_by_project(points: list[Point]) -> dict[str, list[int]]:
    """案件名ごとに点のインデックスをまとめる。件数の多い順に並べる。"""
    groups: dict[str, list[int]] = {}
    for index, point in enumerate(points):
        label = point.project_name or UNASSIGNED_LABEL
        groups.setdefault(label, []).append(index)
    return dict(sorted(groups.items(), key=lambda kv: len(kv[1]), reverse=True))


def save_scatter(
    points: list[Point],
    coords: np.ndarray,
    out_path: Path,
    title: str,
    max_legend: int = 12,
) -> None:
    """散布図を PNG として保存する。

    max_legend を超える案件は凡例に出さない（件数の少ない案件から省く）。
    点は描くが色は共通のグレーにする。
    """
    out_path.parent.mkdir(parents=True, exist_ok=True)
    groups = group_by_project(points)

    figure, axes = plt.subplots(figsize=(12, 10))
    color_map = plt.get_cmap("tab20")
    color_index = 0

    # 未分類を先に描いて背面へ回す。案件の点が隠れないようにするため。
    if UNASSIGNED_LABEL in groups:
        indices = groups.pop(UNASSIGNED_LABEL)
        axes.scatter(
            coords[indices, 0],
            coords[indices, 1],
            s=6,
            c=UNASSIGNED_COLOR,
            alpha=0.4,
            linewidths=0,
            label=f"{UNASSIGNED_LABEL} ({len(indices)})",
        )

    for label, indices in groups.items():
        shown = color_index < max_legend
        axes.scatter(
            coords[indices, 0],
            coords[indices, 1],
            s=14,
            color=color_map(color_index % 20) if shown else "#999999",
            alpha=0.8,
            linewidths=0,
            label=f"{label} ({len(indices)})" if shown else None,
        )
        color_index += 1

    axes.set_title(title)
    axes.set_xlabel("dim 1")
    axes.set_ylabel("dim 2")
    axes.legend(loc="best", fontsize=8, markerscale=1.5)
    figure.tight_layout()
    figure.savefig(out_path, dpi=150)
    plt.close(figure)
```

**日本語フォントについて:** 案件名に日本語が含まれると matplotlib の既定フォントでは豆腐（□）になる。実行時に警告が出るが PNG は生成される。読めない場合は Task 8 の README に記載する対処（`matplotlib.rcParams["font.family"]` の設定）を行う。PoC の目的は「点の塊が分離しているか」の確認であり、凡例が読めれば足りるため、フォント設定は必須要件としない。

- [ ] **Step 4: テストが通ることを確認**

```bash
cd scripts && .venv/bin/python -m pytest tests/test_plot.py -v
```

Expected: 4 passed

- [ ] **Step 5: コミット**

```bash
git add scripts/embedding_viz/plot.py scripts/tests/test_plot.py
git commit -m "feat(scripts): 案件ごとに色分けした散布図の描画を追加"
```

---

### Task 6: CLI エントリポイント

**Files:**
- Create: `scripts/visualize_embeddings.py`

**Interfaces:**
- Consumes: Task 2〜5 のすべて
- Produces: `python visualize_embeddings.py [--db PATH] [--method {pca,tsne,umap}] [--granularity {mail,chunk}] [--limit N] [--include-unassigned] [--out PATH] [--seed N]`

- [ ] **Step 1: 実装する**

このタスクは 4 つの済みモジュールを繋ぐ配線のみで、分岐ロジックを持たない。単体テストは書かず、Step 2 の実行確認をもって検証とする。

`scripts/visualize_embeddings.py`:

```python
#!/usr/bin/env python3
"""Pigeon のメール埋め込みを 2 次元散布図として可視化する。

使い方の詳細は scripts/README.md を参照。

例:
    python visualize_embeddings.py --method pca --limit 1000
    python visualize_embeddings.py --method tsne --granularity chunk
"""

from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path

import numpy as np

from embedding_viz.aggregate import to_chunk_points, to_mail_points
from embedding_viz.db import connect, default_db_path, load_chunks
from embedding_viz.plot import save_scatter
from embedding_viz.reduce import METHODS, reduce_dimensions

# 読み出すチャンク数の既定上限。設計書 §4.4 の「既定 1000 件」に対応する。
# 変更しやすいよう定数として一箇所に置き、--limit で上書きできるようにしている。
DEFAULT_LIMIT = 1000


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--db", type=Path, default=None, help="DB パス（既定: OS ごとの data_dir）"
    )
    parser.add_argument(
        "--method", choices=METHODS, default="tsne", help="次元削減の手法（既定: tsne）"
    )
    parser.add_argument(
        "--granularity",
        choices=("mail", "chunk"),
        default="mail",
        help="点の粒度（既定: mail = チャンクを平均してメール単位にする）",
    )
    # 設計書 §4.4 の「既定 1000 件、いつでも簡単に変えられること」に対応する。
    # ここはチャンク数の上限であり、mail 粒度では 1 メール平均約 3 チャンクなので
    # 点数はおよそ 1/3 になる。数字自体に根拠はないので実データを見ながら調整する。
    parser.add_argument(
        "--limit", type=int, default=DEFAULT_LIMIT, help=f"読み出すチャンク数の上限（既定: {DEFAULT_LIMIT}）"
    )
    parser.add_argument(
        "--include-unassigned",
        action="store_true",
        help="案件未割り当てのメールも含める（既定は除外。実データでは未分類が9割を占め、"
        "含めると案件の点が埋もれるため）",
    )
    parser.add_argument(
        "--out", type=Path, default=None, help="出力 PNG パス（既定: out/<method>-<granularity>.png）"
    )
    parser.add_argument("--seed", type=int, default=42, help="乱数シード（既定: 42）")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    db_path = args.db or default_db_path()

    print(f"DB: {db_path}")
    try:
        conn = connect(db_path)
    except (FileNotFoundError, RuntimeError) as exc:
        print(f"エラー: {exc}", file=sys.stderr)
        return 1

    rows = load_chunks(
        conn, limit=args.limit, assigned_only=not args.include_unassigned
    )
    if not rows:
        print(
            "埋め込み済みのチャンクが 0 件でした。\n"
            "  - Ollama が起動しているか確認してください（未起動だと埋め込みパスが静かに打ち切られます）\n"
            "  - --include-unassigned を付けると未分類メールも対象になります",
            file=sys.stderr,
        )
        return 1
    print(f"チャンク: {len(rows)} 件")

    points = to_mail_points(rows) if args.granularity == "mail" else to_chunk_points(rows)
    print(f"点: {len(points)} 件（粒度: {args.granularity}）")

    if len(points) < 2:
        print("エラー: 次元削減には 2 点以上が必要です", file=sys.stderr)
        return 1

    vectors = np.stack([p.vector for p in points])
    print(f"{args.method} で次元削減中...")
    started = time.perf_counter()
    coords = reduce_dimensions(vectors, method=args.method, seed=args.seed)
    print(f"完了（{time.perf_counter() - started:.1f} 秒）")

    out_path = args.out or Path("out") / f"{args.method}-{args.granularity}.png"
    title = f"{args.method.upper()} / {args.granularity} / {len(points)} points"
    save_scatter(points, coords, out_path, title=title)
    print(f"出力: {out_path.resolve()}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 2: 実 DB に対して実行して PNG が出ることを確認**

まず高速な PCA から:

```bash
cd scripts && .venv/bin/python visualize_embeddings.py --method pca --limit 2000
```

Expected: 以下のような出力と、`scripts/out/pca-mail.png` の生成

```
DB: /Users/<user>/Library/Application Support/Pigeon/pigeon.db
チャンク: 2000 件
点: <N> 件（粒度: mail）
pca で次元削減中...
完了（0.x 秒）
出力: .../scripts/out/pca-mail.png
```

- [ ] **Step 3: 引数が効くことを確認**

```bash
cd scripts && .venv/bin/python visualize_embeddings.py --method pca --granularity chunk --limit 500 --out out/check.png
```

Expected: 「点: 500 件（粒度: chunk）」と表示され `out/check.png` が生成される（チャンク単位なので点数 = チャンク数）

- [ ] **Step 4: 存在しない DB でエラーメッセージが出ることを確認**

```bash
cd scripts && .venv/bin/python visualize_embeddings.py --db /nonexistent/x.db ; echo "exit=$?"
```

Expected: `エラー: DB が見つかりません: /nonexistent/x.db` と `exit=1`

- [ ] **Step 5: コミット**

```bash
git add scripts/visualize_embeddings.py
git commit -m "feat(scripts): 埋め込み可視化のCLIエントリポイントを追加"
```

---

### Task 7: PCA と t-SNE の較正（このプランの本題）

**Files:**
- 変更なし（実行と観察のみ。生成される PNG は `.gitignore` 済み）

**Interfaces:**
- Consumes: Task 6 の CLI
- Produces: 第 2 段階（アプリ内ビュー）へ進んでよいかの判断材料

このタスクは**コードを書かない**。設計書 §3 が定めた分岐判断を行うためのものであり、PoC の存在理由そのものである。

アプリ側は PCA しか持たない設計（Rust で t-SNE / UMAP を実装するのは非現実的なため）なので、**「PCA でどこまで見えるか」を確かめないまま第 2 段階に進むと、作ってから読めない絵しか出ないと分かる**危険がある。

- [ ] **Step 1: 同一条件で PCA と t-SNE を出力する**

```bash
cd scripts && .venv/bin/python visualize_embeddings.py --method pca --limit 3000 --out out/calib-pca.png && .venv/bin/python visualize_embeddings.py --method tsne --limit 3000 --out out/calib-tsne.png
```

Expected: 2 つの PNG が生成される（t-SNE は数十秒かかる）

- [ ] **Step 2: 2 枚を並べて目視で比較する**

```bash
open scripts/out/calib-pca.png scripts/out/calib-tsne.png
```

観察する点:

1. **PCA で案件ごとの塊が見えるか** — これが第 2 段階へ進む条件
2. t-SNE でのみ見える構造があるか
3. 上位 3 案件（Googleの通知 / 勤怠管理 / Atlassian）が分離しているか
4. **上位 3 案件以外の小さな案件はどう見えるか** — 上位 3 件はいずれも送信者が固定された通知メールであり、分離して当然の可能性がある。**bge-m3 が意味を捉えている証拠として弱い。** 件数の少ない案件（イベント・勉強会情報 18 件、SATOラベルプリンター 15 件など）が分離しているかのほうが、指標として意味がある

- [ ] **Step 3: 未分類を含めた場合も確認する**

```bash
cd scripts && .venv/bin/python visualize_embeddings.py --method pca --limit 5000 --include-unassigned --out out/calib-pca-all.png && open scripts/out/calib-pca-all.png
```

観察する点: 未分類の点が案件の塊と重なっているか、それとも別の領域を占めているか。重なっている場合、「本来はその案件に属すべきメールが未分類のまま残っている」可能性を示す

- [ ] **Step 4: 判断を記録する**

`docs/design/2026-07-20-embedding-visualization-design.md` の末尾に「## 11. PoC の較正結果（YYYY-MM-DD）」節を追加し、以下を記録する:

- PCA で案件が分離して見えたか（見えた / 部分的 / 見えない）
- t-SNE との差はどの程度か
- 上位 3 案件を除いた小規模案件の分離状況
- **第 2 段階へ進む判断**: PCA で進む / 設計書 §4.1 の代替案 C（Python が座標を事前計算し DB に保存）へ切り替える

PNG そのものはコミットしない（`.gitignore` 済み。896MB の本番 DB 由来の件名が写り込むため）。**観察した事実を文章で残す。**

- [ ] **Step 5: コミット**

```bash
git add docs/design/2026-07-20-embedding-visualization-design.md
git commit -m "docs(design): 埋め込み可視化PoCの較正結果を記録"
```

---

### Task 8: README

**Files:**
- Create: `scripts/README.md`

**Interfaces:**
- Consumes: Task 1〜7 の成果
- Produces: なし（ドキュメント）

- [ ] **Step 1: README を書く**

`scripts/README.md`:

````markdown
# scripts — 開発用ツール

アプリ本体には含まれない、開発・調査用のスクリプト置き場。

## visualize_embeddings.py — 埋め込み空間の可視化

`vec_chunks` に蓄積された bge-m3 のメール埋め込み（1024 次元）を 2 次元へ落とし、
案件ごとに色分けした散布図を PNG として出力する。

設計: `docs/design/2026-07-20-embedding-visualization-design.md`

### 何を確かめるためのものか

**bge-m3 の埋め込みが捉える意味空間の構造が、実際の案件区分とどれくらい一致するか。**

重要な前提として、**Pigeon の AI 分類は埋め込みを一切使っていない。**
分類は LLM プロンプトベースで、シグナルは送信者アドレス・最近の件名・案件の階層 path である。
埋め込みが使われているのは検索画面のベクトル検索モードのみ。

したがって散布図で案件の色が混ざっていても、それは分類のバグではない。
この可視化は**独立した 2 つの仕組みがどれくらい一致するかの観測**である。

### セットアップ

```bash
cd scripts
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
```

UMAP を使う場合のみ追加で:

```bash
.venv/bin/pip install umap-learn
```

### 実行

```bash
# 既定（t-SNE / メール単位 / 案件割り当て済みのみ / 1000 チャンク）
.venv/bin/python visualize_embeddings.py

# 高速に全体像を見る
.venv/bin/python visualize_embeddings.py --method pca --limit 5000

# チャンク単位で見る（1 メールが複数点に分かれる）
.venv/bin/python visualize_embeddings.py --granularity chunk

# 未分類メールも含める
.venv/bin/python visualize_embeddings.py --include-unassigned
```

出力は既定で `out/<method>-<granularity>.png`。`out/` は `.gitignore` 済み。

### オプション

| オプション | 既定 | 説明 |
|---|---|---|
| `--db PATH` | OS の data_dir | DB パス |
| `--method {pca,tsne,umap}` | `tsne` | 次元削減の手法 |
| `--granularity {mail,chunk}` | `mail` | 点の粒度 |
| `--limit N` | 1000 | 読み出すチャンク数の上限（`visualize_embeddings.py` の `DEFAULT_LIMIT`） |
| `--include-unassigned` | off | 未分類メールも含める |
| `--out PATH` | `out/<method>-<granularity>.png` | 出力先 |
| `--seed N` | 42 | 乱数シード（t-SNE / UMAP の再現性） |

### 手法の使い分け

| 手法 | 速度 | 特徴 |
|---|---|---|
| **PCA** | 速い（1 秒未満） | 線形・決定的。大局的な構造を保つが、クラスタの分離は鮮明でない。**アプリ内ビューはこれを使う予定なので、較正の基準になる** |
| **t-SNE** | 遅い（数十秒〜） | 局所構造を強調しクラスタが分離して見える。**点どうしの距離や塊のサイズに意味はない**（大域的な距離は保存されない） |
| **UMAP** | 中間 | 局所と大域のバランスが良い。追加インストールが必要 |

### 結果を読むときの注意

1. **未分類が大多数を占める。** 2026-07-20 時点でメール 11,531 通のうち案件割り当ては 863 件（7.5%）。
   既定で未分類を除外しているのはこのため。`--include-unassigned` を付けると
   グレーの点が画面を埋め尽くし、案件の点が読めなくなる

2. **上位 3 案件は通知メールで、分離して当然かもしれない。**
   Googleの通知 298 / 勤怠管理 292 / Atlassian 192 で割り当て済みの 9 割を占めるが、
   いずれも送信者が固定された定型メールである。これらが分離していても
   「bge-m3 が意味を捉えた」証拠としては弱い。
   **件数の少ない案件が分離しているかを見るほうが指標として意味がある**

3. **t-SNE の塊の大きさと塊どうしの距離に意味はない。**
   分離しているかどうかだけを見る

4. **DB を読むだけで Ollama は不要。** 埋め込みは生成済み

### トラブルシューティング

**`sqlite_vec` のロードに失敗する**

`vec0` 仮想テーブル（`vec_chunks`）の読み取りには sqlite-vec 拡張が必須。
macOS 標準の Python は拡張ロードが無効化されているため使えない。
mise 管理の Python を使うこと（リポジトリ直下で `mise install`）。

確認方法:

```bash
.venv/bin/python -c "import sqlite3; print(hasattr(sqlite3.connect(':memory:'), 'enable_load_extension'))"
```

`True` が出れば対応している。

**チャンクが 0 件と言われる**

- Ollama が起動しているか確認する。未起動だと埋め込みパスが静かに打ち切られる
  （`AppError::OllamaConnection` で `break` し、キューは保持される）
- `--include-unassigned` を付けると未分類メールも対象になる

**凡例の日本語が豆腐（□）になる**

matplotlib の既定フォントに日本語が無いため。読めなくても点の分離は確認できるので
必須ではないが、直す場合は `embedding_viz/plot.py` の import 直後に追記する:

```python
matplotlib.rcParams["font.family"] = "Hiragino Sans"  # macOS
```

### テスト

```bash
cd scripts && .venv/bin/python -m pytest tests/ -v
```

DB 読み出しと描画そのものは目視確認とし、
集約・次元削減・グルーピングのロジックのみ単体テストで固めている
（間違えても絵が出てしまい、間違いに気づけない箇所のため）。
````

- [ ] **Step 2: README の手順が実際に動くことを確認**

書いたコマンドをそのまま実行して、記載どおりに動くか確かめる:

```bash
cd scripts && .venv/bin/python -m pytest tests/ -v && .venv/bin/python visualize_embeddings.py --method pca --limit 1000
```

Expected: 全テスト passed、PNG 生成

- [ ] **Step 3: コミット**

```bash
git add scripts/README.md
git commit -m "docs(scripts): 埋め込み可視化ツールのREADMEを追加"
```

---

## 完了条件

- [ ] `scripts/` 配下に PoC 一式があり、`pytest tests/ -v` が全て通る
- [ ] 実 DB に対して PCA / t-SNE の両方で PNG が生成できる
- [ ] Task 7 の較正結果が設計書に記録され、第 2 段階へ進むかの判断がついている
- [ ] アプリ本体（`src-tauri/`, `src/`）に変更がない（`git diff --stat main` で確認）

## 次のステップ

Task 7 の判断が「PCA で進む」なら、第 2 段階（Rust PCA + Tauri 独立ウィンドウ + Canvas）のプランを新規に書く。
「代替案 C へ切り替える」なら、設計書 §4.1 を更新してから第 2 段階のプランを書く。
