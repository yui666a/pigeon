import numpy as np
from embedding_viz.aggregate import Point
from embedding_viz.plot import (
    UNASSIGNED_LABEL,
    group_by_project,
    pick_japanese_font,
    save_scatter,
)


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


def test_pick_japanese_font_returns_first_available():
    """優先度リストの先頭に近い候補が Meiryo より優先される。"""
    available = {"Meiryo", "Hiragino Sans", "Arial"}
    assert pick_japanese_font(available) == "Hiragino Sans"


def test_pick_japanese_font_respects_priority_order():
    """mac 系フォントが無い環境でも、タプルの並び順（MS Gothic が Noto より先）を尊重する。"""
    available = {"Noto Sans CJK JP", "MS Gothic"}
    assert pick_japanese_font(available) == "MS Gothic"


def test_pick_japanese_font_returns_none_when_absent():
    available = {"Arial", "DejaVu Sans"}
    assert pick_japanese_font(available) is None


def test_save_scatter_writes_png(tmp_path):
    points = [_point("1", "A"), _point("2", "B")]
    coords = np.array([[0.0, 0.0], [1.0, 1.0]])
    out = tmp_path / "out.png"
    save_scatter(points, coords, out, title="test")
    assert out.exists()
    assert out.stat().st_size > 0
