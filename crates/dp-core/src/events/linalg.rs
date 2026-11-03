//! 4×4 dense solves for the LM normal equations.

pub(super) type M4 = [[f64; 4]; 4];

/// Gaussian elimination with partial pivoting; `None` if numerically singular.
pub(super) fn solve4(mut a: M4, mut b: [f64; 4]) -> Option<[f64; 4]> {
    let scale = a.iter().flatten().fold(0.0f64, |m, x| m.max(x.abs()));
    for col in 0..4 {
        let piv = (col..4).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[piv][col].abs() <= 1e-14 * scale || !a[piv][col].is_finite() {
            return None;
        }
        a.swap(col, piv);
        b.swap(col, piv);
        let pivot = a[col];
        for row in col + 1..4 {
            let m = a[row][col] / pivot[col];
            a[row].iter_mut().zip(pivot).skip(col).for_each(|(x, p)| *x -= m * p);
            b[row] -= m * b[col];
        }
    }
    let mut x = [0.0; 4];
    for row in (0..4).rev() {
        let s: f64 = (row + 1..4).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - s) / a[row][row];
    }
    Some(x)
}

pub(super) fn invert4(a: M4) -> Option<M4> {
    let mut inv = [[0.0; 4]; 4];
    for (j, e) in [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
    .into_iter()
    .enumerate()
    {
        let col = solve4(a, e)?;
        (0..4).for_each(|i| inv[i][j] = col[i]);
    }
    Some(inv)
}
