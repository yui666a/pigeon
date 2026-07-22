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
# 既定（t-SNE / メール単位 / 案件割り当て済みのみ・全件）
.venv/bin/python visualize_embeddings.py

# 高速に全体像を見る
.venv/bin/python visualize_embeddings.py --method pca

# チャンク単位で見る（1 メールが複数点に分かれる）
.venv/bin/python visualize_embeddings.py --granularity chunk

# 未分類メールも含める（このときだけ --limit が効く）
.venv/bin/python visualize_embeddings.py --include-unassigned --limit 5000
```

出力は既定で `out/<method>-<granularity>.png`。`out/` は `.gitignore` 済み。

### オプション

| オプション | 既定 | 説明 |
|---|---|---|
| `--db PATH` | OS の data_dir | DB パス |
| `--method {pca,tsne,umap}` | `tsne` | 次元削減の手法 |
| `--granularity {mail,chunk}` | `mail` | 点の粒度 |
| `--limit N` | 1000 | 読み出すチャンク数の上限（`visualize_embeddings.py` の `DEFAULT_LIMIT`）。**`--include-unassigned` 指定時のみ有効**。案件割り当て済みのみ（既定）のときは、`mail_id` の辞書順切り出しで小さな案件が消えるのを避けるため、常に全件を読む |
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

### 較正結果（2026-07-22）

実 DB（メール 11,531 通 / 案件割り当て 863 件）に対して PCA と t-SNE を目視で比較した。
**PCA でも上位 3 案件（Googleの通知・勤怠管理・Atlassian）に加え、件数の少ない案件
（SATOラベルプリンター・イベント/勉強会情報・freee会計）まで分離が確認できた。**
これは「上位案件は送信者固定の通知メールだから分離して当然」という懸念（上記の注意 2）
に対する反証で、bge-m3 が送信者だけでなく内容の意味的な類似を捉えていることを示す。

この結果を受け、**アプリ側の第 2 段階は PCA を採用する。**
詳細（観察の全項目・判断の根拠）は
`docs/design/2026-07-20-embedding-visualization-design.md` §11 を参照。

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

`plot.py` はモジュール読み込み時に、インストール済みフォントから日本語対応フォントを
自動選択する（`pick_japanese_font`）。候補は OS ごとに以下の優先順位で探索する:

| OS | 候補 |
|---|---|
| macOS | Hiragino Sans / Hiragino Maru Gothic Pro / YuGothic |
| Windows | Yu Gothic / Meiryo / MS Gothic |
| Linux | Noto Sans CJK JP / IPAexGothic / TakaoGothic |

いずれもインストールされていない場合は既定フォント（DejaVu Sans）のままになり、
凡例の日本語は豆腐になるが、点の分離自体は確認できるため処理は継続する
（描画は失敗しない）。直す場合は候補のいずれかをインストールする。
Linux の例: `sudo apt install fonts-noto-cjk`

### テスト

```bash
cd scripts && .venv/bin/python -m pytest tests/ -v
```

DB 読み出しと描画そのものは目視確認とし、
集約・次元削減・グルーピング・フォント選択のロジックのみ単体テストで固めている
（間違えても絵が出てしまい、間違いに気づけない箇所のため）。
