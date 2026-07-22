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
