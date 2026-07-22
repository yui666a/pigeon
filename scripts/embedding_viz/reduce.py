"""高次元の埋め込みを 2 次元へ落とす。

手法の使い分け:
  pca  — 線形・高速・決定的。大局的な構造を保つが、クラスタの分離は鮮明でない
  tsne — 局所構造を強調しクラスタが分離して見える。遅く、大域的な距離は信用できない
  umap — 中間。局所と大域のバランスが良いが umap-learn の追加インストールが必要
"""

from __future__ import annotations

import numpy as np
from sklearn.decomposition import PCA
from sklearn.manifold import TSNE

METHODS: tuple[str, ...] = ("pca", "tsne", "umap")


def reduce_dimensions(
    vectors: np.ndarray, method: str, seed: int = 42
) -> np.ndarray:
    """(N, D) の配列を (N, 2) へ落とす。"""
    if vectors.shape[0] < 2:
        raise ValueError(
            f"次元削減には 2 点以上が必要です（入力: {vectors.shape[0]} 点）"
        )

    # float64 へ寄せる。sklearn は内部で float64 を使うため、
    # 明示しておくと手法ごとの微妙な数値差を避けられる。
    data = np.asarray(vectors, dtype=np.float64)

    if method == "pca":
        # PCA は内部で中心化を行う。決定的なので seed は不要。
        return PCA(n_components=2, random_state=seed).fit_transform(data)

    if method == "tsne":
        # perplexity は点数より小さくないと sklearn がエラーを出す。
        # 既定 30 を、少数データでも動くよう安全側へ丸める。
        perplexity = min(30.0, max(5.0, (data.shape[0] - 1) / 3.0))
        return TSNE(
            n_components=2,
            random_state=seed,
            perplexity=perplexity,
            init="pca",
        ).fit_transform(data)

    if method == "umap":
        try:
            import umap
        except ImportError as exc:
            raise RuntimeError(
                "umap-learn がインストールされていません。"
                "`pip install umap-learn` を実行するか、--method pca / tsne を使ってください。"
            ) from exc
        n_neighbors = min(15, max(2, data.shape[0] - 1))
        return umap.UMAP(
            n_components=2, random_state=seed, n_neighbors=n_neighbors
        ).fit_transform(data)

    raise ValueError(f"不明な手法です: {method}（利用可能: {', '.join(METHODS)}）")
