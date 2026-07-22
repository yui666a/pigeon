"""2 次元座標を案件ごとに色分けした散布図として描く。"""

from __future__ import annotations

from pathlib import Path

import matplotlib

# GUI バックエンドを避ける（ヘッドレス環境と CI で落ちないように）。
# pyplot の import より前に設定する必要がある。
matplotlib.use("Agg")

import matplotlib.font_manager  # noqa: E402

# 日本語対応の候補フォント（プラットフォーム別）。先に書いたものが優先される。
_JP_FONT_CANDIDATES = (
    "Hiragino Sans",  # macOS
    "Hiragino Maru Gothic Pro",
    "YuGothic",  # macOS/Windows
    "Yu Gothic",  # Windows
    "Meiryo",  # Windows
    "MS Gothic",  # Windows
    "Noto Sans CJK JP",  # Linux
    "IPAexGothic",  # Linux
    "TakaoGothic",  # Linux
)


def pick_japanese_font(available: set[str]) -> str | None:
    """`available` の中から優先度順で最初に見つかった日本語対応フォント名を返す。

    `available` は matplotlib のフォントマネージャから得たインストール済みフォント名の集合。
    グローバル状態に触れない純粋関数にして、ユニットテストしやすくしている。
    """
    for name in _JP_FONT_CANDIDATES:
        if name in available:
            return name
    return None


# 案件名の日本語が凡例で豆腐（□）にならないよう、インストール済みフォントから自動選択する。
# 見つからない場合は既定フォントのままにし、処理は継続する（豆腐にはなるが致命的ではない）。
_available_fonts = {f.name for f in matplotlib.font_manager.fontManager.ttflist}
_jp_font = pick_japanese_font(_available_fonts)
if _jp_font is not None:
    matplotlib.rcParams["font.family"] = _jp_font
matplotlib.rcParams["axes.unicode_minus"] = False

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
