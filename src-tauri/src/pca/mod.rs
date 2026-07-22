//! 高次元ベクトルを 2 次元へ落とすべき乗法 PCA。DB に依存しない純粋計算。
//!
//! 上位 2 主成分だけが必要なので共分散行列（D×D）は作らず、べき乗法で
//! 直接求める。計算量は O(N·D·反復) で、N が数千・D=1024 でも 1 秒未満。
//! 第 2 主成分は第 1 主成分をデフレーションしてから同手順で求める。

use crate::error::AppError;

/// べき乗法の反復回数。実データ（bge-m3, 分離が明瞭）では 30 で十分収束する。
const ITERATIONS: usize = 50;

/// N×D の行列を 2 次元へ射影する。各行が 1 つの点。
pub fn project_2d(vectors: &[Vec<f32>]) -> Result<Vec<(f32, f32)>, AppError> {
    let n = vectors.len();
    if n < 2 {
        return Err(AppError::Validation(format!(
            "PCA には 2 点以上が必要です（入力: {n} 点）"
        )));
    }
    let dim = vectors[0].len();

    // 中心化: 各次元の平均を引く。中心化を忘れると主成分が原点方向に歪む。
    let mut mean = vec![0.0f64; dim];
    for v in vectors {
        for (m, &x) in mean.iter_mut().zip(v.iter()) {
            *m += x as f64;
        }
    }
    for m in mean.iter_mut() {
        *m /= n as f64;
    }
    let centered: Vec<Vec<f64>> = vectors
        .iter()
        .map(|v| v.iter().zip(&mean).map(|(&x, &m)| x as f64 - m).collect())
        .collect();

    let pc1 = principal_axis(&centered, dim, None);
    let pc2 = principal_axis(&centered, dim, Some(&pc1));

    // 各点を 2 軸へ射影
    let coords = centered
        .iter()
        .map(|row| {
            let x = dot(row, &pc1);
            let y = dot(row, &pc2);
            (x as f32, y as f32)
        })
        .collect();
    Ok(coords)
}

/// べき乗法で主成分（単位ベクトル）を 1 本求める。
/// `deflate` が与えられたら、その成分を各反復で除去して直交する軸を得る。
fn principal_axis(centered: &[Vec<f64>], dim: usize, deflate: Option<&[f64]>) -> Vec<f64> {
    // 初期ベクトル: 全次元 1 で開始（決定的にするため乱数を使わない）。
    let mut axis = vec![1.0f64 / (dim as f64).sqrt(); dim];

    for _ in 0..ITERATIONS {
        // y = Cov * axis を、共分散行列を作らず y = Σ_i x_i (x_i · axis) で計算
        let mut next = vec![0.0f64; dim];
        for row in centered {
            let proj = dot(row, &axis);
            for (n, &x) in next.iter_mut().zip(row.iter()) {
                *n += x * proj;
            }
        }
        // 第 2 軸を求めるときは第 1 軸成分を除去（直交化）
        if let Some(prev) = deflate {
            let overlap = dot(&next, prev);
            for (n, &p) in next.iter_mut().zip(prev.iter()) {
                *n -= overlap * p;
            }
        }
        // 正規化
        let norm = dot(&next, &next).sqrt();
        if norm < 1e-12 {
            // 分散が消えた（全点同一など）。現在の軸を返して打ち切る。
            break;
        }
        for n in next.iter_mut() {
            *n /= norm;
        }
        axis = next;
    }
    axis
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(&x, &y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 明確に 2 軸方向へ伸びた点群。PC1 は分散最大の軸を向くはず。
    fn two_axis_data() -> Vec<Vec<f32>> {
        // x 軸に大きく、y 軸に小さく散らばる 3 次元データ（残り 1 次元はゼロ）
        vec![
            vec![-10.0, -1.0, 0.0],
            vec![-5.0, 0.5, 0.0],
            vec![0.0, 0.0, 0.0],
            vec![5.0, -0.5, 0.0],
            vec![10.0, 1.0, 0.0],
        ]
    }

    #[test]
    fn returns_one_point_per_input() {
        let coords = project_2d(&two_axis_data()).unwrap();
        assert_eq!(coords.len(), 5);
    }

    #[test]
    fn pc1_captures_dominant_axis() {
        // PC1（射影後の x 座標）は入力第0次元の順序を保つはず（単調）。
        let coords = project_2d(&two_axis_data()).unwrap();
        let xs: Vec<f32> = coords.iter().map(|c| c.0).collect();
        // 入力が第0次元で単調増加なので、PC1 も単調（増加か減少）になる
        let increasing = xs.windows(2).all(|w| w[0] <= w[1]);
        let decreasing = xs.windows(2).all(|w| w[0] >= w[1]);
        assert!(
            increasing || decreasing,
            "PC1 は支配軸に沿って単調になるはず: {xs:?}"
        );
    }

    #[test]
    fn pc1_spread_exceeds_pc2_spread() {
        // 分散最大の軸が PC1 に来るので、x のばらつき > y のばらつき
        let coords = project_2d(&two_axis_data()).unwrap();
        let spread = |vals: Vec<f32>| {
            let mean = vals.iter().sum::<f32>() / vals.len() as f32;
            vals.iter().map(|v| (v - mean).powi(2)).sum::<f32>()
        };
        let xs = spread(coords.iter().map(|c| c.0).collect());
        let ys = spread(coords.iter().map(|c| c.1).collect());
        assert!(
            xs > ys * 5.0,
            "PC1 の分散が PC2 より十分大きいはず: x={xs} y={ys}"
        );
    }

    #[test]
    fn errors_on_too_few_points() {
        let one = vec![vec![1.0, 2.0, 3.0]];
        assert!(project_2d(&one).is_err());
    }

    #[test]
    fn is_deterministic() {
        let a = project_2d(&two_axis_data()).unwrap();
        let b = project_2d(&two_axis_data()).unwrap();
        assert_eq!(a, b);
    }

    /// 平均が非ゼロのデータ。中心化を忘れると主成分が原点方向へ歪むため、
    /// 中心化ステップが実際に効いていることを検証する。
    fn shifted_two_axis_data() -> Vec<Vec<f32>> {
        // two_axis_data を全次元 +100 平行移動（分散構造は同じ、平均だけ非ゼロ）
        two_axis_data()
            .into_iter()
            .map(|row| row.iter().map(|&v| v + 100.0).collect())
            .collect()
    }

    #[test]
    fn centering_makes_shift_invariant() {
        // 中心化が効いていれば、全体を平行移動しても射影後の相対配置は不変。
        // 中心化を忘れると平行移動で主成分の向きが変わり、この不変性が壊れる。
        let base = project_2d(&two_axis_data()).unwrap();
        let shifted = project_2d(&shifted_two_axis_data()).unwrap();
        // PC1 座標の点間差分（相対配置）が一致することを確認
        for i in 1..base.len() {
            let d_base = base[i].0 - base[i - 1].0;
            let d_shift = shifted[i].0 - shifted[i - 1].0;
            assert!(
                (d_base.abs() - d_shift.abs()).abs() < 1e-2,
                "中心化が効いていれば平行移動で相対配置は不変: base={d_base} shift={d_shift}"
            );
        }
    }
}
