# 埋め込みマップ Phase A（見る）実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** メール埋め込みを Rust の PCA で 2D に落とし、Tauri 独立ウィンドウの Canvas 散布図として案件ごとに色分け表示し、点クリックで軽量な本文プレビューを出す。

**Architecture:** Rust 側で `vec_chunks` から生ベクトル(bytes→f32)を読み、メール単位 centroid に集約し、べき乗法 PCA で 2D 座標を算出して返す新規 command を作る。フロントは Vite の第2エントリ `visualization.html` として独立ウィンドウの中身を持ち、素の Canvas に散布図を描く。点クリックで軽量プレビュー command を叩く。

**Tech Stack:** Rust (rusqlite, zerocopy, 自前べき乗法PCA、線形代数クレートなし) / React 19 + TypeScript + Vite + Canvas 2D / Tauri 2 WebviewWindow

## この計画の位置づけ

設計書 `docs/design/2026-07-22-embedding-map-window-design.md` の §9 に従い、機能を 2 フェーズに分ける。本計画は **Phase A（見る）**のみ:
- Rust PCA + command + 別ウィンドウ + Canvas 描画 + 点クリック軽量プレビュー

**Phase B（片付ける）**= D&D 割り当て + 案件パネル + イベント同期は、Phase A 完了後に別計画として書く。Phase A だけで「マップを見て分類の健康診断ができる」動くソフトになる（設計書 §1 の第二目的）。第一目的の「未分類の発見と片付け」は Phase B で完成する。

## Global Constraints

- **設計書と ADR を正とする**。横断的判断は `docs/adr/` を優先
- `unwrap()` / `expect()` はテストコード以外で使わない。アプリエラーは `thiserror`。Tauri commands は `Result<T, AppError>` を返す（`AppError` は `src-tauri/src/error.rs`）
- **1024 次元の生ベクトルをフロントに渡さない**。Rust 側で 2D まで落とし座標のみ返す（設計書 §4.2）
- **線形代数クレート（nalgebra 等）を導入しない**。上位 2 主成分はべき乗法で自前計算（設計書 §4.2）
- 埋め込み次元は 1024（bge-m3）。`vec_chunks.embedding` は `zerocopy::IntoBytes` の生バイト列（`src-tauri/src/db/chunks.rs`）で、リトルエンディアン f32×1024
- 案件色は既存規則 `project.color ?? "#6b7280"` を踏襲（設計書 §4.6）。自動パレットは作らない
- `any` を使わない。Tauri invoke のレスポンスに型を付ける。共通型は `src/types/`
- コミットは Conventional Commits。scope は `ui` / `db` / `search` 等
- Rust: `cargo test` / `cargo fmt`。フロント: `pnpm test`（Vitest）。**`cargo fmt` はリポジトリ全体を整形しうるので、変更したファイルのみ `git add` する**

## 前提知識（既存コードの接続点、実地確認済み 2026-07-23）

- **command 登録**: `src-tauri/src/lib.rs:299` の `tauri::generate_handler![...]` 配列に 1 行追加。モジュールは `src-tauri/src/commands/mod.rs` に `pub mod`
- **read 系 command の雛形**: `src-tauri/src/commands/directory_commands.rs:53` — `#[tauri::command] pub fn xxx(db: State<DbState>, ...) -> Result<T, AppError> { db.with_conn(|conn| ...) }`。`DbState` は `src-tauri/src/state.rs:11`、`.manage` 済み
- **単体メール取得**: `src-tauri/src/db/mails.rs:113` `get_mail_by_id(conn: &Connection, mail_id: &str) -> Result<Mail, AppError>`（既存、再利用可）
- **vec_chunks の読み**: 既存は距離のみ（`vec_search.rs:27`）。**生ベクトル全件読みは新規**。`SELECT embedding FROM vec_chunks` が vec0 から BLOB を返すので `chunks_exact(4)` + `f32::from_le_bytes` で復元
- **JOIN 構造**（PoC で確定）: `vec_chunks v` → `mail_chunks c ON c.id = v.chunk_id` → `mails m ON m.id = c.mail_id` → `LEFT JOIN mail_project_assignments mpa ON mpa.mail_id = m.id` → `LEFT JOIN projects p ON p.id = mpa.project_id`
- **Vite**: `vite.config.ts` は単一エントリ（`rollupOptions.input` なし）。第2エントリ追加は新規
- **tauri.conf.json**: `app.windows` は 1 定義で **label 未指定（既定 "main"）**。`capabilities/default.json` は `windows: ["main"]`。マルチウィンドウは新規配線
- **イベント実績**: Rust `app.emit("embed-progress", ...)`（`mail_commands.rs:31`）、フロント `listen<T>("...", cb)`（`accountStore.ts:114`）

## File Structure

| ファイル | 責務 |
|---|---|
| `src-tauri/src/db/embedding_map.rs`（新規） | `vec_chunks` から生ベクトル+ラベルを全件読み、bytes→f32 復元 |
| `src-tauri/src/pca/mod.rs`（新規） | べき乗法 PCA（centroid 集約 → 上位2主成分 → 2D射影）。DB 非依存の純粋計算 |
| `src-tauri/src/commands/embedding_map_commands.rs`（新規） | command `embedding_map_points` / `mail_preview` |
| `src-tauri/src/lib.rs`（修正） | command 登録、db/pca モジュール宣言 |
| `src-tauri/src/db/mod.rs` / `commands/mod.rs`（修正） | `pub mod` 追加 |
| `visualization.html`（新規） | 独立ウィンドウの HTML エントリ |
| `src/visualization.tsx`（新規） | 独立ウィンドウの React エントリ |
| `vite.config.ts`（修正） | 第2入力 `visualization.html` を追加 |
| `src-tauri/tauri.conf.json` / `capabilities/default.json`（修正） | window label と新ウィンドウ capability |
| `src/components/embedding-map/EmbeddingMapCanvas.tsx`（新規） | Canvas 散布図（描画・ズーム・パン・ホバー・クリック） |
| `src/components/embedding-map/mapGeometry.ts`（新規） | 座標→スクリーン変換・ヒットテスト（純粋関数、テスト対象） |
| `src/components/embedding-map/PreviewPane.tsx`（新規） | 軽量本文プレビュー |
| `src/api/embeddingMapApi.ts`（新規） | command のフロント型付きラッパー |
| `src/types/embeddingMap.ts`（新規） | `MapPoint` / `MailPreview` 型 |
| メイン側にウィンドウを開く導線（`src/components/sidebar/` 付近、修正） | ボタン + `WebviewWindow` 生成 |

**なぜ pca を db から分離するか**: PCA は DB に依存しない純粋計算で、合成データで厳密にテストできる（間違えても絵が出てしまう種類のロジック）。DB 読みと分けることで、PCA 単体を検証できる。

---

### Task 1: べき乗法 PCA（純粋計算、Rust）

**Files:**
- Create: `src-tauri/src/pca/mod.rs`
- Modify: `src-tauri/src/lib.rs`（`mod pca;` 追加。既存の `mod` 宣言群の並びに合わせる）

**Interfaces:**
- Consumes: なし（`Vec<Vec<f32>>` を受ける純粋関数）
- Produces:
  - `pub fn project_2d(vectors: &[Vec<f32>]) -> Result<Vec<(f32, f32)>, AppError>` — N×D を受け N 個の (x,y) を返す。中心化 → べき乗法で PC1/PC2 → 射影
  - 2 点未満は `AppError`

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/pca/mod.rs` の末尾に付けるテスト（先にファイルを作り、テストだけ書く）:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// 明確に 2 軸方向へ伸びた点群。PC1 は分散最大の軸を向くはず。
    fn two_axis_data() -> Vec<Vec<f32>> {
        // x 軸に大きく、y 軸に小さく散らばる 3 次元データ（残り 1 次元はゼロ）
        vec![
            vec![-10.0, -1.0, 0.0],
            vec![-5.0, 0.5, 0.0],
            vec![0.0, 0.0, 0.0],
            vec![5.0, -0.5, 0.0],
            vec![10.0, 1.0, 0.0],
        ]
    }

    #[test]
    fn returns_one_point_per_input() {
        let coords = project_2d(&two_axis_data()).unwrap();
        assert_eq!(coords.len(), 5);
    }

    #[test]
    fn pc1_captures_dominant_axis() {
        // PC1（射影後の x 座標）は入力第0次元の順序を保つはず（単調）。
        let coords = project_2d(&two_axis_data()).unwrap();
        let xs: Vec<f32> = coords.iter().map(|c| c.0).collect();
        // 入力が第0次元で単調増加なので、PC1 も単調（増加か減少）になる
        let increasing = xs.windows(2).all(|w| w[0] <= w[1]);
        let decreasing = xs.windows(2).all(|w| w[0] >= w[1]);
        assert!(increasing || decreasing, "PC1 は支配軸に沿って単調になるはず: {xs:?}");
    }

    #[test]
    fn pc1_spread_exceeds_pc2_spread() {
        // 分散最大の軸が PC1 に来るので、x のばらつき > y のばらつき
        let coords = project_2d(&two_axis_data()).unwrap();
        let spread = |vals: Vec<f32>| {
            let mean = vals.iter().sum::<f32>() / vals.len() as f32;
            vals.iter().map(|v| (v - mean).powi(2)).sum::<f32>()
        };
        let xs = spread(coords.iter().map(|c| c.0).collect());
        let ys = spread(coords.iter().map(|c| c.1).collect());
        assert!(xs > ys * 5.0, "PC1 の分散が PC2 より十分大きいはず: x={xs} y={ys}");
    }

    #[test]
    fn errors_on_too_few_points() {
        let one = vec![vec![1.0, 2.0, 3.0]];
        assert!(project_2d(&one).is_err());
    }

    #[test]
    fn is_deterministic() {
        let a = project_2d(&two_axis_data()).unwrap();
        let b = project_2d(&two_axis_data()).unwrap();
        assert_eq!(a, b);
    }
}
```

- [ ] **Step 2: テストが失敗（コンパイルエラー）することを確認**

まず `src-tauri/src/lib.rs` に `mod pca;` を追加（他の `mod xxx;` 宣言と同じ場所）。
`src-tauri/src/pca/mod.rs` に上のテストだけ書いた状態で:

Run: `cd src-tauri && cargo test --lib pca:: 2>&1 | tail -20`
Expected: FAIL — `cannot find function project_2d`

- [ ] **Step 3: 実装する**

`src-tauri/src/pca/mod.rs` の先頭（テストモジュールの前）に:

```rust
//! 高次元ベクトルを 2 次元へ落とすべき乗法 PCA。DB に依存しない純粋計算。
//!
//! 上位 2 主成分だけが必要なので共分散行列（D×D）は作らず、べき乗法で
//! 直接求める。計算量は O(N·D·反復) で、N が数千・D=1024 でも 1 秒未満。
//! 第 2 主成分は第 1 主成分をデフレーションしてから同手順で求める。

use crate::error::AppError;

/// べき乗法の反復回数。実データ（bge-m3, 分離が明瞭）では 30 で十分収束する。
const ITERATIONS: usize = 50;

/// N×D の行列を 2 次元へ射影する。各行が 1 つの点。
pub fn project_2d(vectors: &[Vec<f32>]) -> Result<Vec<(f32, f32)>, AppError> {
    let n = vectors.len();
    if n < 2 {
        return Err(AppError::Validation(format!(
            "PCA には 2 点以上が必要です（入力: {n} 点）"
        )));
    }
    let dim = vectors[0].len();

    // 中心化: 各次元の平均を引く。中心化を忘れると主成分が原点方向に歪む。
    let mut mean = vec![0.0f64; dim];
    for v in vectors {
        for (m, &x) in mean.iter_mut().zip(v.iter()) {
            *m += x as f64;
        }
    }
    for m in mean.iter_mut() {
        *m /= n as f64;
    }
    let centered: Vec<Vec<f64>> = vectors
        .iter()
        .map(|v| v.iter().zip(&mean).map(|(&x, &m)| x as f64 - m).collect())
        .collect();

    let pc1 = principal_axis(&centered, dim, None);
    let pc2 = principal_axis(&centered, dim, Some(&pc1));

    // 各点を 2 軸へ射影
    let coords = centered
        .iter()
        .map(|row| {
            let x = dot(row, &pc1);
            let y = dot(row, &pc2);
            (x as f32, y as f32)
        })
        .collect();
    Ok(coords)
}

/// べき乗法で主成分（単位ベクトル）を 1 本求める。
/// `deflate` が与えられたら、その成分を各反復で除去して直交する軸を得る。
fn principal_axis(centered: &[Vec<f64>], dim: usize, deflate: Option<&[f64]>) -> Vec<f64> {
    // 初期ベクトル: 全次元 1 で開始（決定的にするため乱数を使わない）。
    let mut axis = vec![1.0f64 / (dim as f64).sqrt(); dim];

    for _ in 0..ITERATIONS {
        // y = Cov * axis を、共分散行列を作らず y = Σ_i x_i (x_i · axis) で計算
        let mut next = vec![0.0f64; dim];
        for row in centered {
            let proj = dot(row, &axis);
            for (n, &x) in next.iter_mut().zip(row.iter()) {
                *n += x * proj;
            }
        }
        // 第 2 軸を求めるときは第 1 軸成分を除去（直交化）
        if let Some(prev) = deflate {
            let overlap = dot(&next, prev);
            for (n, &p) in next.iter_mut().zip(prev.iter()) {
                *n -= overlap * p;
            }
        }
        // 正規化
        let norm = dot(&next, &next).sqrt();
        if norm < 1e-12 {
            // 分散が消えた（全点同一など）。現在の軸を返して打ち切る。
            break;
        }
        for n in next.iter_mut() {
            *n /= norm;
        }
        axis = next;
    }
    axis
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(&x, &y)| x * y).sum()
}
```

`AppError::Validation`（`src-tauri/src/error.rs`、`#[error("Validation error: {0}")]`）を入力エラーに使う。これはコードベース全体で入力検証エラーに使われている既存バリアントで、「2 点未満」もこれに該当する。新規バリアントは追加しない（Task 1 のレビューで、重複する `InvalidInput` を追加せず既存 `Validation` に統一する方針に確定済み）。

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib pca:: 2>&1 | tail -20`
Expected: 5 tests passed

- [ ] **Step 5: フォーマットとコミット**

```bash
cd src-tauri && cargo fmt -- src/pca/mod.rs
cd .. && git add src-tauri/src/pca/mod.rs src-tauri/src/lib.rs
# error.rs を触った場合はそれも add
git commit -m "feat(search): 埋め込み用のべき乗法PCAを追加"
```

---

### Task 2: vec_chunks からの生ベクトル読み出し（Rust, DB）

**Files:**
- Create: `src-tauri/src/db/embedding_map.rs`
- Modify: `src-tauri/src/db/mod.rs`（`pub mod embedding_map;` 追加）

**Interfaces:**
- Consumes: なし
- Produces:
  - `pub struct MapChunkRow { pub mail_id: String, pub subject: String, pub project_id: Option<String>, pub project_name: Option<String>, pub project_color: Option<String>, pub vector: Vec<f32> }`
  - `pub fn load_map_chunks(conn: &Connection) -> Result<Vec<MapChunkRow>, AppError>` — 全チャンクの生ベクトル + ラベルを読む。1024 次元でない行はスキップ
  - `pub fn decode_embedding(blob: &[u8]) -> Result<Vec<f32>, AppError>` — bytes→f32×1024

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/db/embedding_map.rs` の末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_embedding_roundtrips_le_f32() {
        let original: Vec<f32> = vec![0.5, -1.25, 3.0, 0.0];
        let mut blob = Vec::new();
        for f in &original {
            blob.extend_from_slice(&f.to_le_bytes());
        }
        assert_eq!(decode_embedding(&blob).unwrap(), original);
    }

    #[test]
    fn decode_embedding_rejects_misaligned_length() {
        // f32 は 4 バイト境界。5 バイトは壊れたデータ。
        assert!(decode_embedding(&[0u8; 5]).is_err());
    }
}
```

`load_map_chunks` は実 DB / vec0 拡張が要るため単体テストは書かず、Task 3 の command 実行で実データ検証する（PoC と同じ方針: 生バイト復元だけ単体テスト）。

- [ ] **Step 2: テストが失敗することを確認**

まず `src-tauri/src/db/mod.rs` に `pub mod embedding_map;` を追加。
`embedding_map.rs` に下の実装なし・テストだけの状態で:

Run: `cd src-tauri && cargo test --lib embedding_map:: 2>&1 | tail -15`
Expected: FAIL — `cannot find function decode_embedding`

- [ ] **Step 3: 実装する**

`src-tauri/src/db/embedding_map.rs` の先頭:

```rust
//! 埋め込みマップ用に vec_chunks から生ベクトルとラベルを全件読み出す。
//! 既存の vec_search は距離しか読まないため、生ベクトルの復元はここが唯一。

use crate::error::AppError;
use rusqlite::Connection;

/// bge-m3 の次元数。vec_chunks の float[1024] に対応（migrations v18）。
const EMBEDDING_DIM: usize = 1024;

pub struct MapChunkRow {
    pub mail_id: String,
    pub subject: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub project_color: Option<String>,
    pub vector: Vec<f32>,
}

/// vec_chunks.embedding の生バイト列を f32 配列へ復元する。
/// 書き込みは zerocopy::IntoBytes（chunks.rs）でリトルエンディアン。
pub fn decode_embedding(blob: &[u8]) -> Result<Vec<f32>, AppError> {
    if blob.len() % 4 != 0 {
        return Err(AppError::Validation(format!(
            "f32 境界に揃っていません: {} バイト",
            blob.len()
        )));
    }
    Ok(blob
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}

/// 全チャンクの生ベクトルとラベルを読む。JOIN 構造は vec_search を踏襲。
/// 次元が EMBEDDING_DIM でない行はスキップする（再埋め込み途中など）。
pub fn load_map_chunks(conn: &Connection) -> Result<Vec<MapChunkRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT v.embedding, c.mail_id, m.subject, mpa.project_id, p.name, p.color
         FROM vec_chunks v
         JOIN mail_chunks c ON c.id = v.chunk_id
         JOIN mails m ON m.id = c.mail_id
         LEFT JOIN mail_project_assignments mpa ON mpa.mail_id = m.id
         LEFT JOIN projects p ON p.id = mpa.project_id
         ORDER BY c.mail_id, c.chunk_index",
    )?;
    let rows = stmt.query_map([], |row| {
        let blob: Vec<u8> = row.get(0)?;
        Ok((
            blob,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;

    let mut result = Vec::new();
    let mut skipped = 0usize;
    for row in rows {
        let (blob, mail_id, subject, project_id, project_name, project_color) = row?;
        let vector = decode_embedding(&blob)?;
        if vector.len() != EMBEDDING_DIM {
            skipped += 1;
            continue;
        }
        result.push(MapChunkRow {
            mail_id,
            subject: subject.unwrap_or_else(|| "(件名なし)".to_string()),
            project_id,
            project_name,
            project_color,
            vector,
        });
    }
    if skipped > 0 {
        eprintln!("警告: 次元が {EMBEDDING_DIM} でない行を {skipped} 件スキップ");
    }
    Ok(result)
}
```

注意: `m.subject` を `Option<String>` で受けているのは NULL 許容の保険。`mails.subject` が NOT NULL なら `String` で受けてよいが、`Option` + `unwrap_or_else` の方が壊れない。

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib embedding_map:: 2>&1 | tail -15`
Expected: 2 tests passed

- [ ] **Step 5: フォーマットとコミット**

```bash
cd src-tauri && cargo fmt -- src/db/embedding_map.rs
cd .. && git add src-tauri/src/db/embedding_map.rs src-tauri/src/db/mod.rs
git commit -m "feat(db): vec_chunksから生ベクトルを全件読み出す関数を追加"
```

---

### Task 3: 座標算出 command（centroid 集約 + PCA を束ねる）

**Files:**
- Create: `src-tauri/src/commands/embedding_map_commands.rs`
- Modify: `src-tauri/src/commands/mod.rs`（`pub mod embedding_map_commands;`）、`src-tauri/src/lib.rs`（generate_handler に `embedding_map_points` 追加）

**Interfaces:**
- Consumes: `db::embedding_map::{load_map_chunks, MapChunkRow}`（Task 2）、`pca::project_2d`（Task 1）
- Produces:
  - `#[derive(serde::Serialize)] pub struct MapPoint { pub x: f32, pub y: f32, pub mail_id: String, pub subject: String, pub project_id: Option<String>, pub project_name: Option<String>, pub project_color: Option<String> }`
  - command `embedding_map_points(db: State<DbState>) -> Result<Vec<MapPoint>, AppError>`

- [ ] **Step 1: 失敗するテストを書く**

集約ロジック（チャンク → メール centroid）を純粋関数に切り出してテストする。`embedding_map_commands.rs` の末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::embedding_map::MapChunkRow;

    fn row(mail_id: &str, vector: Vec<f32>) -> MapChunkRow {
        MapChunkRow {
            mail_id: mail_id.to_string(),
            subject: format!("件名{mail_id}"),
            project_id: Some("p1".to_string()),
            project_name: Some("案件A".to_string()),
            project_color: Some("#ff0000".to_string()),
            vector,
        }
    }

    #[test]
    fn aggregates_chunks_of_same_mail_by_centroid() {
        let rows = vec![row("m1", vec![1.0, 0.0]), row("m1", vec![0.0, 2.0])];
        let mails = aggregate_by_mail(rows);
        assert_eq!(mails.len(), 1);
        // centroid は要素ごとの平均 [0.5, 1.0]
        assert_eq!(mails[0].vector, vec![0.5, 1.0]);
        assert_eq!(mails[0].mail_id, "m1");
    }

    #[test]
    fn keeps_distinct_mails_separate() {
        let rows = vec![row("m1", vec![1.0, 0.0]), row("m2", vec![0.0, 1.0])];
        let mails = aggregate_by_mail(rows);
        assert_eq!(mails.len(), 2);
    }

    #[test]
    fn preserves_label_from_first_chunk() {
        let rows = vec![row("m1", vec![1.0, 0.0]), row("m1", vec![0.0, 2.0])];
        let mails = aggregate_by_mail(rows);
        assert_eq!(mails[0].project_color.as_deref(), Some("#ff0000"));
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

`commands/mod.rs` に `pub mod embedding_map_commands;` を追加し、テストだけの状態で:

Run: `cd src-tauri && cargo test --lib embedding_map_commands:: 2>&1 | tail -15`
Expected: FAIL — `cannot find function aggregate_by_mail`

- [ ] **Step 3: 実装する**

`src-tauri/src/commands/embedding_map_commands.rs` の先頭:

```rust
//! 埋め込みマップの座標算出 command。
//! 生ベクトル読み出し（db::embedding_map）→ メール単位 centroid 集約 →
//! べき乗法 PCA（pca::project_2d）を束ね、2D 座標 + ラベルを返す。

use crate::db::embedding_map::{load_map_chunks, MapChunkRow};
use crate::error::AppError;
use crate::pca::project_2d;
use crate::state::DbState;
use tauri::State;

#[derive(serde::Serialize)]
pub struct MapPoint {
    pub x: f32,
    pub y: f32,
    pub mail_id: String,
    pub subject: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub project_color: Option<String>,
}

/// メール単位に集約した中間表現（ラベル + centroid ベクトル）。
struct MailAgg {
    mail_id: String,
    subject: String,
    project_id: Option<String>,
    project_name: Option<String>,
    project_color: Option<String>,
    vector: Vec<f32>,
}

/// 同一メールのチャンクを centroid（要素ごとの平均）へ集約する。
/// ラベルは最初のチャンクのものを採用（同一メール内で同じ）。
/// 入力の並び順（mail_id, chunk_index）を保つため、初出順を維持する。
fn aggregate_by_mail(rows: Vec<MapChunkRow>) -> Vec<MailAgg> {
    use std::collections::HashMap;
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<MapChunkRow>> = HashMap::new();
    for r in rows {
        if !groups.contains_key(&r.mail_id) {
            order.push(r.mail_id.clone());
        }
        groups.entry(r.mail_id.clone()).or_default().push(r);
    }

    order
        .into_iter()
        .map(|mail_id| {
            let group = groups.remove(&mail_id).expect("order にある mail_id は groups にある");
            let dim = group[0].vector.len();
            let mut centroid = vec![0.0f32; dim];
            for r in &group {
                for (c, &x) in centroid.iter_mut().zip(r.vector.iter()) {
                    *c += x;
                }
            }
            let count = group.len() as f32;
            for c in centroid.iter_mut() {
                *c /= count;
            }
            let head = &group[0];
            MailAgg {
                mail_id,
                subject: head.subject.clone(),
                project_id: head.project_id.clone(),
                project_name: head.project_name.clone(),
                project_color: head.project_color.clone(),
                vector: centroid,
            }
        })
        .collect()
}

#[tauri::command]
pub fn embedding_map_points(db: State<DbState>) -> Result<Vec<MapPoint>, AppError> {
    let rows = db.with_conn(load_map_chunks)?;
    let mails = aggregate_by_mail(rows);
    if mails.len() < 2 {
        return Err(AppError::Validation(
            "埋め込みマップには 2 通以上の埋め込み済みメールが必要です".to_string(),
        ));
    }
    let vectors: Vec<Vec<f32>> = mails.iter().map(|m| m.vector.clone()).collect();
    let coords = project_2d(&vectors)?;

    let points = mails
        .into_iter()
        .zip(coords)
        .map(|(m, (x, y))| MapPoint {
            x,
            y,
            mail_id: m.mail_id,
            subject: m.subject,
            project_id: m.project_id,
            project_name: m.project_name,
            project_color: m.project_color,
        })
        .collect();
    Ok(points)
}
```

注意: `aggregate_by_mail` 内の `.expect(...)` はテストコードではないが、直前に `order` に入れた mail_id が必ず `groups` にある不変条件が成立するため理論上到達不能。気になる場合は `filter_map` + `continue` で回避してよいが、ロジック上は安全。**Global Constraints の unwrap/expect 禁止を厳守するなら**、`groups.remove(...).ok_or_else(|| AppError::Validation("集約の不変条件違反".into()))?` にして `map` を `collect::<Result<Vec<_>,_>>()` にする形へ変える。実装者はどちらか選ぶ（後者が規約に忠実）。

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib embedding_map_commands:: 2>&1 | tail -15`
Expected: 3 tests passed

- [ ] **Step 5: command を登録する**

`src-tauri/src/lib.rs:299` の `generate_handler![` 配列の末尾（`bulk_move_mails` の次の行）に追加:

```rust
            commands::bulk_commands::bulk_move_mails,
            commands::embedding_map_commands::embedding_map_points,
        ])
```

- [ ] **Step 6: 全体ビルドと実 DB 検証**

Run: `cd src-tauri && cargo build 2>&1 | tail -10`
Expected: ビルド成功（警告のみ可）

実データ検証は次の Task 5（フロント）でウィンドウから叩くため、ここではビルドが通ればよい。手早く確認したい場合はアプリを起動し devtools から `invoke("embedding_map_points")` を呼ぶ。

- [ ] **Step 7: フォーマットとコミット**

```bash
cd src-tauri && cargo fmt -- src/commands/embedding_map_commands.rs
cd .. && git add src-tauri/src/commands/embedding_map_commands.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat(search): 埋め込みマップの座標算出commandを追加"
```

---

### Task 4: 軽量本文プレビュー command

**Files:**
- Modify: `src-tauri/src/commands/embedding_map_commands.rs`（`mail_preview` 追加）、`src-tauri/src/lib.rs`（登録）

**Interfaces:**
- Consumes: `db::mails::get_mail_by_id`（既存, `src-tauri/src/db/mails.rs:113`）
- Produces:
  - `#[derive(serde::Serialize)] pub struct MailPreview { pub mail_id: String, pub subject: String, pub from_addr: String, pub date: String, pub body_excerpt: String }`
  - command `mail_preview(db: State<DbState>, mail_id: String) -> Result<MailPreview, AppError>`

- [ ] **Step 1: 失敗するテストを書く**

本文冒頭の切り出しを純粋関数にしてテストする。`embedding_map_commands.rs` のテストモジュールに追加:

```rust
    #[test]
    fn excerpt_truncates_long_body() {
        let body = "あ".repeat(1000);
        let out = body_excerpt(Some(&body), 300);
        assert_eq!(out.chars().count(), 300);
    }

    #[test]
    fn excerpt_handles_none_body() {
        assert_eq!(body_excerpt(None, 300), "");
    }

    #[test]
    fn excerpt_keeps_short_body() {
        assert_eq!(body_excerpt(Some("短い本文"), 300), "短い本文");
    }
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test --lib embedding_map_commands::tests::excerpt 2>&1 | tail -15`
Expected: FAIL — `cannot find function body_excerpt`

- [ ] **Step 3: 実装する**

`embedding_map_commands.rs` に追加（use と struct、関数）:

```rust
use crate::db::mails::get_mail_by_id;

#[derive(serde::Serialize)]
pub struct MailPreview {
    pub mail_id: String,
    pub subject: String,
    pub from_addr: String,
    pub date: String,
    pub body_excerpt: String,
}

/// 本文テキストの先頭を最大 max_chars 文字で切り出す（文字境界を尊重）。
fn body_excerpt(body: Option<&str>, max_chars: usize) -> String {
    match body {
        None => String::new(),
        Some(text) => text.chars().take(max_chars).collect(),
    }
}

#[tauri::command]
pub fn mail_preview(db: State<DbState>, mail_id: String) -> Result<MailPreview, AppError> {
    let mail = db.with_conn(|conn| get_mail_by_id(conn, &mail_id))?;
    Ok(MailPreview {
        mail_id: mail.id,
        subject: mail.subject,
        from_addr: mail.from_addr,
        date: mail.date,
        body_excerpt: body_excerpt(mail.body_text.as_deref(), 300),
    })
}
```

`Mail` 構造体のフィールド名（`id`, `subject`, `from_addr`, `date`, `body_text: Option<String>`）は `src/types/mail.ts` と対応する Rust 側 `src-tauri/src/models/mail.rs`（または `db/mails.rs`）で確認する。フィールド名が違えば合わせる。

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test --lib embedding_map_commands:: 2>&1 | tail -15`
Expected: 6 tests passed（Task3 の 3 + 本 Task の 3）

- [ ] **Step 5: command 登録・ビルド・コミット**

`lib.rs` の generate_handler に `commands::embedding_map_commands::mail_preview,` を追加。

```bash
cd src-tauri && cargo build 2>&1 | tail -5 && cargo fmt -- src/commands/embedding_map_commands.rs
cd .. && git add src-tauri/src/commands/embedding_map_commands.rs src-tauri/src/lib.rs
git commit -m "feat(search): 埋め込みマップ用の軽量メールプレビューcommandを追加"
```

---

### Task 5: 独立ウィンドウの土台（Vite 第2エントリ + Tauri window 配線）

**Files:**
- Create: `visualization.html`
- Create: `src/visualization.tsx`
- Modify: `vite.config.ts`（`rollupOptions.input` に 2 エントリ）
- Modify: `src-tauri/tauri.conf.json`（main に `"label": "main"` を明示）
- Modify: `src-tauri/capabilities/default.json`（windows に新ラベル、window 生成 permission）

**Interfaces:**
- Consumes: なし
- Produces: `embedding-map` ラベルのウィンドウで `visualization.html` を開ける状態

- [ ] **Step 1: Vite に第2エントリを追加**

`vite.config.ts` の `plugins` の後に `build` を足す:

```ts
export default defineConfig(async () => ({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  build: {
    rollupOptions: {
      input: {
        main: "index.html",
        visualization: "visualization.html",
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
}));
```

- [ ] **Step 2: HTML と React エントリを作る**

`visualization.html`（リポジトリ直下、`index.html` と同じ階層）。`index.html` を開いて `<title>` と `<script src>` 以外を踏襲する:

```html
<!doctype html>
<html lang="ja">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>埋め込みマップ</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/visualization.tsx"></script>
  </body>
</html>
```

`src/visualization.tsx`（`src/main.tsx` を開いてスタイル import と mount 方法を踏襲）:

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import "./index.css"; // main.tsx が読むグローバル CSS に合わせる（main.tsx を確認して同じものを import）

function VisualizationRoot() {
  return <div style={{ padding: 16 }}>埋め込みマップ（準備中）</div>;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <VisualizationRoot />
  </React.StrictMode>,
);
```

`src/main.tsx` を開き、CSS の import 名（`./index.css` か別名か）を確認して合わせる。

- [ ] **Step 3: tauri.conf.json の main に label を明示**

`src-tauri/tauri.conf.json` の `app.windows[0]` に `"label": "main"` を追加（他フィールドは変えない）:

```json
      {
        "label": "main",
        "title": "Pigeon",
        "width": 1280,
        "height": 800,
        "minWidth": 900,
        "minHeight": 600,
        "center": true
      }
```

embedding-map ウィンドウは静的定義せず、フロントから動的に `WebviewWindow` で生成する（Task 6）。そのため conf に第2 window は書かない。

- [ ] **Step 4: capability を更新**

`src-tauri/capabilities/default.json` の `windows` に `embedding-map` を追加し、ウィンドウ生成・イベントの permission を足す:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for the main and embedding-map windows",
  "windows": ["main", "embedding-map"],
  "permissions": [
    "core:default",
    "core:window:allow-create",
    "core:webview:allow-create-webview-window",
    "opener:default",
    "deep-link:default",
    "dialog:default",
    "notification:default"
  ]
}
```

permission 名は Tauri 2 の実際のスキーマに合わせる。`cargo build` 時に `gen/schemas` で検証されるので、名前が違えばビルドエラーで判明する。正しい名前は `src-tauri/gen/schemas/desktop-schema.json` を grep（`window:allow-create` / `webview-window` 相当）して確認する。

- [ ] **Step 5: ビルドが通ることを確認**

Run: `pnpm build 2>&1 | tail -15`
Expected: Vite ビルド成功。`dist/visualization.html` が生成される

Run: `cd src-tauri && cargo build 2>&1 | tail -10`
Expected: capability の permission 名が正しければ成功。失敗したら permission 名を schema に合わせて修正

- [ ] **Step 6: コミット**

```bash
git add visualization.html src/visualization.tsx vite.config.ts src-tauri/tauri.conf.json src-tauri/capabilities/default.json
git commit -m "feat(ui): 埋め込みマップ用の独立ウィンドウの土台を追加"
```

---

### Task 6: ウィンドウを開く導線（メイン側）

**Files:**
- Modify: メイン UI の適切な場所（未分類ビュー付近）。`src/components/sidebar/` 配下か `App.tsx`。実装者が既存の未分類導線（`viewMode: "unclassified"`）の近くを選ぶ

**Interfaces:**
- Consumes: `@tauri-apps/api/webviewWindow` の `WebviewWindow`
- Produces: ボタンクリックで `embedding-map` ウィンドウを開く/フォーカスする

- [ ] **Step 1: ウィンドウを開くユーティリティを書く**

`src/components/embedding-map/openMapWindow.ts`（新規）:

```ts
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

const MAP_LABEL = "embedding-map";

/**
 * 埋め込みマップウィンドウを開く。既に開いていればフォーカスするだけ。
 * 多重起動を防ぐため必ずこの関数を通す。
 */
export async function openEmbeddingMapWindow(): Promise<void> {
  const existing = await WebviewWindow.getByLabel(MAP_LABEL);
  if (existing) {
    await existing.setFocus();
    return;
  }
  const win = new WebviewWindow(MAP_LABEL, {
    url: "visualization.html",
    title: "埋め込みマップ",
    width: 1100,
    height: 800,
  });
  win.once("tauri://error", (e) => {
    console.error("埋め込みマップウィンドウの生成に失敗", e);
  });
}
```

`@tauri-apps/api/webviewWindow` の API 名（`WebviewWindow.getByLabel`, コンストラクタ, `url` オプション）は導入済みの `@tauri-apps/api` バージョンで確認する（`node_modules/@tauri-apps/api/webviewWindow.d.ts` を見る）。名前が違えば合わせる。

- [ ] **Step 2: ボタンを置く**

未分類ビューのヘッダ付近（`src/components/thread-list/UnclassifiedList.tsx` のヘッダか、サイドバー）に、既存のボタンスタイルに倣って「マップで見る」ボタンを追加し、`onClick={() => openEmbeddingMapWindow()}` を付ける。既存ボタン（例: `ClassifyButton.tsx`）の className を踏襲する。

- [ ] **Step 3: 手動確認**

Run: `pnpm tauri dev`（起動に時間がかかる。別の確認手段があればそれで可）
確認: 未分類ビューの「マップで見る」ボタン → 新ウィンドウが開き「埋め込みマップ（準備中）」が出る。もう一度押しても二重に開かない（フォーカスのみ）。

自動テストは Task 5/6 のウィンドウ配線には現実的でないため、この手動確認をもって検証とする。

- [ ] **Step 4: コミット**

```bash
git add src/components/embedding-map/openMapWindow.ts <ボタンを足したファイル>
git commit -m "feat(ui): 未分類ビューから埋め込みマップを開く導線を追加"
```

---

### Task 7: 座標→スクリーン変換とヒットテスト（純粋関数、フロント）

**Files:**
- Create: `src/components/embedding-map/mapGeometry.ts`
- Create: `src/types/embeddingMap.ts`
- Test: `src/components/embedding-map/mapGeometry.test.ts`

**Interfaces:**
- Consumes: なし
- Produces:
  - `src/types/embeddingMap.ts`: `export interface MapPoint { x: number; y: number; mail_id: string; subject: string; project_id: string | null; project_name: string | null; project_color: string | null }`、`export interface MailPreview { mail_id: string; subject: string; from_addr: string; date: string; body_excerpt: string }`
  - `mapGeometry.ts`: `computeBounds(points)`, `makeTransform(bounds, width, height, padding)`, `worldToScreen(t, x, y)`, `hitTest(points, t, sx, sy, radius)`

- [ ] **Step 1: 失敗するテストを書く**

`src/components/embedding-map/mapGeometry.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { computeBounds, makeTransform, worldToScreen, hitTest } from "./mapGeometry";
import type { MapPoint } from "../../types/embeddingMap";

const pt = (x: number, y: number, id = "m"): MapPoint => ({
  x, y, mail_id: id, subject: "s", project_id: null, project_name: null, project_color: null,
});

describe("computeBounds", () => {
  it("returns min/max over points", () => {
    const b = computeBounds([pt(-1, 2), pt(3, -4)]);
    expect(b).toEqual({ minX: -1, maxX: 3, minY: -4, maxY: 2 });
  });
});

describe("makeTransform + worldToScreen", () => {
  it("maps world bounds into the padded canvas box", () => {
    const b = { minX: 0, maxX: 10, minY: 0, maxY: 10 };
    const t = makeTransform(b, 100, 100, 10); // padding 10 → 描画領域 80x80
    // 左下(0,0) は左下隅(10, 90) に、右上(10,10) は右上隅(90,10) に。
    expect(worldToScreen(t, 0, 0)).toEqual({ sx: 10, sy: 90 });
    expect(worldToScreen(t, 10, 10)).toEqual({ sx: 90, sy: 10 });
  });
});

describe("hitTest", () => {
  it("returns the nearest point within radius", () => {
    const b = { minX: 0, maxX: 10, minY: 0, maxY: 10 };
    const t = makeTransform(b, 100, 100, 10);
    const points = [pt(0, 0, "a"), pt(10, 10, "b")];
    // 画面座標(10,90) 付近をクリック → a に当たる
    expect(hitTest(points, t, 11, 89, 5)?.mail_id).toBe("a");
  });

  it("returns null when nothing is within radius", () => {
    const b = { minX: 0, maxX: 10, minY: 0, maxY: 10 };
    const t = makeTransform(b, 100, 100, 10);
    expect(hitTest([pt(0, 0)], t, 50, 50, 5)).toBeNull();
  });
});
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm test src/components/embedding-map/mapGeometry.test.ts 2>&1 | tail -15`
Expected: FAIL — モジュール未解決

- [ ] **Step 3: 実装する**

`src/types/embeddingMap.ts`:

```ts
export interface MapPoint {
  x: number;
  y: number;
  mail_id: string;
  subject: string;
  project_id: string | null;
  project_name: string | null;
  project_color: string | null;
}

export interface MailPreview {
  mail_id: string;
  subject: string;
  from_addr: string;
  date: string;
  body_excerpt: string;
}
```

`src/components/embedding-map/mapGeometry.ts`:

```ts
import type { MapPoint } from "../../types/embeddingMap";

export interface Bounds {
  minX: number;
  maxX: number;
  minY: number;
  maxY: number;
}

export interface Transform {
  scale: number;
  offsetX: number;
  offsetY: number;
  height: number;
}

export function computeBounds(points: MapPoint[]): Bounds {
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
  for (const p of points) {
    if (p.x < minX) minX = p.x;
    if (p.x > maxX) maxX = p.x;
    if (p.y < minY) minY = p.y;
    if (p.y > maxY) maxY = p.y;
  }
  return { minX, maxX, minY, maxY };
}

/**
 * world 座標を padding 付きの canvas ボックスへ等方スケールで収める変換を作る。
 * y は画面下向きが正なので上下反転する。
 */
export function makeTransform(
  b: Bounds,
  width: number,
  height: number,
  padding: number,
): Transform {
  const spanX = b.maxX - b.minX || 1;
  const spanY = b.maxY - b.minY || 1;
  const boxW = width - padding * 2;
  const boxH = height - padding * 2;
  const scale = Math.min(boxW / spanX, boxH / spanY);
  // world minX,minY を左下（padding, height-padding）に合わせる
  const offsetX = padding - b.minX * scale;
  const offsetY = padding - b.minY * scale;
  return { scale, offsetX, offsetY, height };
}

export function worldToScreen(t: Transform, x: number, y: number): { sx: number; sy: number } {
  const sx = x * t.scale + t.offsetX;
  // y 反転: world 上方向 → 画面上方向
  const sy = t.height - (y * t.scale + t.offsetY);
  return { sx, sy };
}

/**
 * 画面座標(sx,sy) に最も近い点を radius(px) 以内で返す。無ければ null。
 */
export function hitTest(
  points: MapPoint[],
  t: Transform,
  sx: number,
  sy: number,
  radius: number,
): MapPoint | null {
  let best: MapPoint | null = null;
  let bestDist = radius * radius;
  for (const p of points) {
    const s = worldToScreen(t, p.x, p.y);
    const dx = s.sx - sx;
    const dy = s.sy - sy;
    const d = dx * dx + dy * dy;
    if (d <= bestDist) {
      bestDist = d;
      best = p;
    }
  }
  return best;
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `pnpm test src/components/embedding-map/mapGeometry.test.ts 2>&1 | tail -15`
Expected: 4 tests passed

- [ ] **Step 5: コミット**

```bash
git add src/types/embeddingMap.ts src/components/embedding-map/mapGeometry.ts src/components/embedding-map/mapGeometry.test.ts
git commit -m "feat(ui): 埋め込みマップの座標変換とヒットテストを追加"
```

---

### Task 8: Canvas 散布図 + プレビュー（ウィンドウの中身を完成）

**Files:**
- Create: `src/api/embeddingMapApi.ts`
- Create: `src/components/embedding-map/EmbeddingMapCanvas.tsx`
- Create: `src/components/embedding-map/PreviewPane.tsx`
- Modify: `src/visualization.tsx`（準備中プレースホルダを実画面に差し替え）

**Interfaces:**
- Consumes: `MapPoint`/`MailPreview`（Task 7）、`mapGeometry`（Task 7）、command `embedding_map_points`/`mail_preview`（Task 3/4）
- Produces: 動く散布図ウィンドウ

- [ ] **Step 1: API ラッパーを書く**

`src/api/embeddingMapApi.ts`（既存 `src/api/mailApi.ts` の `invokeCommand` を踏襲。`mailApi.ts` を開いて import 元を確認）:

```ts
import { invokeCommand } from "./invoke"; // mailApi.ts と同じ invoke ラッパーを使う
import type { MapPoint, MailPreview } from "../types/embeddingMap";

export const embeddingMapApi = {
  points: () => invokeCommand<MapPoint[]>("embedding_map_points", {}),
  preview: (mailId: string) =>
    invokeCommand<MailPreview>("mail_preview", { mailId }),
};
```

`invokeCommand` の import 元は `mailApi.ts` の先頭行に合わせる（`./invoke` でない可能性がある）。command 引数のキー名（`mailId` か `mail_id` か）は Tauri の慣習に従い、既存 `mailApi.ts` の `bulkMoveMails`（`{ mailIds, projectId }` = camelCase）に倣って **camelCase**。Rust 側は `mail_id: String` 引数だが Tauri が camelCase↔snake_case を変換する。

- [ ] **Step 2: プレビューペインを書く**

`src/components/embedding-map/PreviewPane.tsx`:

```tsx
import type { MailPreview } from "../../types/embeddingMap";

interface Props {
  preview: MailPreview | null;
  loading: boolean;
}

export function PreviewPane({ preview, loading }: Props) {
  if (loading) return <div className="p-4 text-sm text-gray-500">読み込み中...</div>;
  if (!preview)
    return <div className="p-4 text-sm text-gray-400">点をクリックするとメールの概要が出ます</div>;
  return (
    <div className="p-4 space-y-2 overflow-y-auto">
      <div className="font-semibold text-sm">{preview.subject}</div>
      <div className="text-xs text-gray-500">{preview.from_addr}</div>
      <div className="text-xs text-gray-400">{preview.date}</div>
      <div className="text-sm whitespace-pre-wrap border-t pt-2">{preview.body_excerpt}</div>
    </div>
  );
}
```

- [ ] **Step 3: Canvas コンポーネントを書く**

`src/components/embedding-map/EmbeddingMapCanvas.tsx`:

```tsx
import { useEffect, useRef } from "react";
import type { MapPoint } from "../../types/embeddingMap";
import { computeBounds, makeTransform, worldToScreen, hitTest, type Transform } from "./mapGeometry";

const PADDING = 40;
const UNASSIGNED_COLOR = "#cccccc";
const DEFAULT_PROJECT_COLOR = "#6b7280";

interface Props {
  points: MapPoint[];
  onPointClick: (mailId: string) => void;
}

export function EmbeddingMapCanvas({ points, onPointClick }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const transformRef = useRef<Transform | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const width = canvas.width;
    const height = canvas.height;
    const bounds = computeBounds(points);
    const t = makeTransform(bounds, width, height, PADDING);
    transformRef.current = t;

    ctx.clearRect(0, 0, width, height);

    // 未分類を先に背面へ（薄いグレー・小さめ）。案件を覆い隠さないため。
    for (const p of points) {
      if (p.project_id) continue;
      const s = worldToScreen(t, p.x, p.y);
      ctx.fillStyle = UNASSIGNED_COLOR;
      ctx.globalAlpha = 0.4;
      ctx.beginPath();
      ctx.arc(s.sx, s.sy, 2, 0, Math.PI * 2);
      ctx.fill();
    }
    // 案件を前面へ（色付き・大きめ）
    for (const p of points) {
      if (!p.project_id) continue;
      const s = worldToScreen(t, p.x, p.y);
      ctx.fillStyle = p.project_color ?? DEFAULT_PROJECT_COLOR;
      ctx.globalAlpha = 0.85;
      ctx.beginPath();
      ctx.arc(s.sx, s.sy, 3.5, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.globalAlpha = 1;
  }, [points]);

  const handleClick = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const t = transformRef.current;
    const canvas = canvasRef.current;
    if (!t || !canvas) return;
    const rect = canvas.getBoundingClientRect();
    const sx = e.clientX - rect.left;
    const sy = e.clientY - rect.top;
    const hit = hitTest(points, t, sx, sy, 6);
    if (hit) onPointClick(hit.mail_id);
  };

  return (
    <canvas
      ref={canvasRef}
      width={800}
      height={800}
      onClick={handleClick}
      className="bg-white"
    />
  );
}
```

注意: ズーム/パンは Phase A では省く（YAGNI — まず表示と点クリックを確認する。設計書 §10 の確認項目にズーム/パンはあるが、Phase A の最小で価値が出るのは「見える + クリックできる」。ズーム/パンは Task を分けて本 Task の後に足すか、Phase B に回す）。**この判断を実装者が勝手に変えないこと** — 固定 800×800 で全体表示、クリックのみ。

- [ ] **Step 4: ウィンドウ本体を組む**

`src/visualization.tsx` の `VisualizationRoot` を差し替え:

```tsx
import { useEffect, useState } from "react";
import { embeddingMapApi } from "./api/embeddingMapApi";
import { EmbeddingMapCanvas } from "./components/embedding-map/EmbeddingMapCanvas";
import { PreviewPane } from "./components/embedding-map/PreviewPane";
import type { MapPoint, MailPreview } from "./types/embeddingMap";

function VisualizationRoot() {
  const [points, setPoints] = useState<MapPoint[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<MailPreview | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);

  useEffect(() => {
    embeddingMapApi.points().then(setPoints).catch((e) => setError(String(e)));
  }, []);

  const handlePointClick = (mailId: string) => {
    setPreviewLoading(true);
    embeddingMapApi
      .preview(mailId)
      .then(setPreview)
      .catch((e) => setError(String(e)))
      .finally(() => setPreviewLoading(false));
  };

  if (error) return <div className="p-4 text-red-600">エラー: {error}</div>;

  return (
    <div className="flex h-screen">
      <div className="flex-1 flex items-center justify-center overflow-hidden">
        <EmbeddingMapCanvas points={points} onPointClick={handlePointClick} />
      </div>
      <div className="w-80 border-l overflow-y-auto">
        <PreviewPane preview={preview} loading={previewLoading} />
      </div>
    </div>
  );
}
```

（`VisualizationRoot` の宣言は残し、中身だけ差し替え。既存の `ReactDOM.createRoot(...)` はそのまま）

- [ ] **Step 5: ビルドと実データ確認**

Run: `pnpm build 2>&1 | tail -8`
Expected: 成功

Run: `pnpm tauri dev` で起動 → 未分類ビューの「マップで見る」→ ウィンドウに散布図が出る。
確認項目:
- 案件ごとに色が付いた点、未分類は薄いグレー背面
- 点クリック → 右に件名・送信者・本文冒頭が出る
- エラー時（埋め込み 0 件など）はエラーメッセージ

`pnpm test 2>&1 | tail -5` で既存テストが壊れていないことも確認。

- [ ] **Step 6: コミット**

```bash
git add src/api/embeddingMapApi.ts src/components/embedding-map/EmbeddingMapCanvas.tsx src/components/embedding-map/PreviewPane.tsx src/visualization.tsx
git commit -m "feat(ui): 埋め込みマップのCanvas散布図と本文プレビューを追加"
```

---

## 完了条件（Phase A）

- [ ] `cargo test` / `pnpm test` が全て通る
- [ ] 実データでマップウィンドウが開き、案件ごとに色分けされた散布図が出る
- [ ] 未分類の点が背面グレーで、案件の点が前面に見える
- [ ] 点クリックで軽量プレビュー（件名・送信者・本文冒頭）が出る
- [ ] アプリ本体（メインウィンドウ）の既存挙動が壊れていない

## Phase A で意図的に見送ったもの（Phase B / 後続）

- **ズーム / パン**: Phase A は固定 800×800 全体表示。密集部の拡大は後続
- **D&D による案件割り当て**: Phase B
- **案件パネル（ドロップ先）**: Phase B
- **イベント同期（emit/listen）**: Phase B（割り当てが無いので Phase A では不要）
- **件数フィルタ**: 実データを見てから判断（設計書 §4.3）

## 次のステップ

Phase A 完了・レビュー後、Phase B（D&D 割り当て + 案件パネル + イベント同期）の計画を書く。Phase B は本 Phase の `embedding-map` ウィンドウ・`MapPoint`・`bulk_move_mails` 直 invoke を土台にする。
