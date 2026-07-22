import numpy as np
import pytest
from embedding_viz.reduce import reduce_dimensions


def _two_cluster_data(n=30, dim=50, seed=0):
    """明確に離れた 2 クラスタを作る。次元削減後も分離が保たれるはず。"""
    rng = np.random.default_rng(seed)
    a = rng.normal(loc=0.0, scale=0.1, size=(n, dim))
    b = rng.normal(loc=10.0, scale=0.1, size=(n, dim))
    return np.vstack([a, b]).astype(np.float32)


def test_pca_returns_two_columns():
    coords = reduce_dimensions(_two_cluster_data(), method="pca")
    assert coords.shape == (60, 2)


def test_pca_preserves_cluster_separation():
    """PCA が正しく動けば、2 クラスタは第 1 主成分上で分離する。

    中心化を忘れると分離が崩れるので、それを検出できる。
    """
    data = _two_cluster_data()
    coords = reduce_dimensions(data, method="pca")
    first_half, second_half = coords[:30, 0], coords[30:, 0]
    # クラスタ内のばらつきより、クラスタ間の距離のほうが十分大きいこと
    gap = abs(first_half.mean() - second_half.mean())
    spread = max(first_half.std(), second_half.std())
    assert gap > spread * 5


def test_pca_is_deterministic():
    data = _two_cluster_data()
    assert np.allclose(
        reduce_dimensions(data, method="pca"), reduce_dimensions(data, method="pca")
    )


def test_tsne_returns_two_columns():
    coords = reduce_dimensions(_two_cluster_data(), method="tsne", seed=1)
    assert coords.shape == (60, 2)


def test_tsne_is_reproducible_with_same_seed():
    data = _two_cluster_data()
    a = reduce_dimensions(data, method="tsne", seed=7)
    b = reduce_dimensions(data, method="tsne", seed=7)
    assert np.allclose(a, b)


def test_unknown_method_raises():
    with pytest.raises(ValueError, match="不明な手法"):
        reduce_dimensions(_two_cluster_data(), method="magic")


def test_too_few_points_raises():
    """点が 2 未満だと主成分が定義できない。"""
    with pytest.raises(ValueError, match="2 点以上"):
        reduce_dimensions(np.zeros((1, 50), dtype=np.float32), method="pca")
