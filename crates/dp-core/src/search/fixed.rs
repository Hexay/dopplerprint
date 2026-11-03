//! Fixed-point arithmetic of the f0 sweep, shared bit for bit by the CPU sweep (sweep.rs) and the CUDA
//! kernel (gpu/sweep.cu), with no FP64 in the inner loop (consumer GPUs run FP64 at 1/64 rate).
//!
//! - Positions (log-frequency bins) are i64 in units of 2^-POS_SHIFT bin. A position is
//!   `b0_start + s·step + offset[frame] + shift[harmonic]`, all pre-rounded with `pos`, so it is exact and
//!   order-independent; the integer part picks the bin, the top 24 fraction bits the lerp weight.
//! - Each term `w·lerp` (f32, no FMA) is truncated toward zero to a multiple of 2^-ACC_SHIFT (one
//!   cvttss2si / __float2int_rz; round-half-even is a libm call on baseline x86-64) and summed in i64:
//!   exact and associative, so any summation order gives the same total. Requires
//!   |w·plane| < 2^(31−ACC_SHIFT) (z_clip·max weight; default 8·1); beyond it both sides saturate.
//! - z = (total · 2^-ACC_SHIFT) / norm in f64, once per f0 step.

pub const POS_SHIFT: u32 = 32;
pub const ACC_SHIFT: u32 = 24;
/// Offset of a frame the track does not cover.
pub const NONE: i64 = i64::MIN;

/// f64 bins → fixed-point position (NaN/∞ → NONE).
pub fn pos(x: f64) -> i64 {
    if x.is_finite() { (x * (1u64 << POS_SHIFT) as f64).round() as i64 } else { NONE }
}

/// Exclusive upper bound of in-row positions: the last bin pair (n_f−2, n_f−1).
pub fn top(n_f: usize) -> i64 {
    (n_f.saturating_sub(1) as i64) << POS_SHIFT
}

/// Linear interpolation at fixed-point `p`, `0 ≤ p < top(row.len())`.
pub fn lerp(row: &[f32], p: i64) -> f32 {
    let j = (p >> POS_SHIFT) as usize;
    let fr = ((p >> (POS_SHIFT - 24)) & 0xff_ffff) as f32 * (1.0 / 16_777_216.0);
    row[j] * (1.0 - fr) + row[j + 1] * fr
}

/// One term of the sum, truncated toward zero to 2^-ACC_SHIFT.
pub fn term(v: f32) -> i64 {
    (v * (1u32 << ACC_SHIFT) as f32) as i32 as i64
}

pub fn total(acc: i64) -> f64 {
    acc as f64 * (1.0 / (1u64 << ACC_SHIFT) as f64)
}

/// Steps s in `0..n` with `0 ≤ base + s·step < top`, as a range (`step > 0`; positions rise with s).
pub fn in_row(base: i64, step: i64, top: i64, n: usize) -> std::ops::Range<usize> {
    let first = |bound: i64| -> usize {
        // smallest s ≥ 0 with base + s·step ≥ bound
        if base >= bound { 0 } else { ((bound - base + step - 1) / step).min(n as i64) as usize }
    };
    first(0)..first(top).max(first(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_row_matches_scan() {
        let (step, top_) = (pos(0.25), top(40));
        for base in [-pos(20.0) - 3, -pos(0.25), -1, 0, 1, pos(10.3), top_ - 1, top_, top_ + 5] {
            let n = 300;
            let want: Vec<usize> = (0..n).filter(|&s| (0..top_).contains(&(base + s as i64 * step))).collect();
            let r = in_row(base, step, top_, n);
            assert_eq!(r.clone().collect::<Vec<_>>(), want, "base {base}");
        }
    }

    #[test]
    fn term_truncates_toward_zero() {
        let q = 1.0 / (1u32 << ACC_SHIFT) as f32;
        assert_eq!(term(2.5 * q), 2);
        assert_eq!(term(3.9 * q), 3);
        assert_eq!(term(-2.5 * q), -2);
        assert_eq!(term(1e9), i32::MAX as i64);
        assert_eq!(term(7.75), 31 << (ACC_SHIFT - 2));
    }
}
