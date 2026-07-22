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
