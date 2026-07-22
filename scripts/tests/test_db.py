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


def test_build_query_orders_assigned_first_for_limit():
    """--include-unassigned + --limit のとき、割り当て済みの点が LIMIT で
    間引かれないよう、割り当て済みを先に並べる（I-1）。"""
    sql, _ = build_query(limit=1000, assigned_only=False)
    assert "ORDER BY (mpa.project_id IS NULL)" in sql
    # 割り当て済み優先の並びが、mail_id によるタイブレークより先に来ること
    # （SELECT 句にも c.mail_id が出るため、ORDER BY 句だけを見て比較する）。
    order_by_clause = sql[sql.index("ORDER BY") :]
    assert order_by_clause.index("mpa.project_id IS NULL") < order_by_clause.index(
        "c.mail_id"
    )
