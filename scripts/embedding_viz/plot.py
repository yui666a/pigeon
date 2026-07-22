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
