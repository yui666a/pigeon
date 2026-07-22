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
