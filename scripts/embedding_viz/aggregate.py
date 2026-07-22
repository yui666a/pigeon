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
