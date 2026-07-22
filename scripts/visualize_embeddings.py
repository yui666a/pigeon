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
