// f0 sweep on the GPU; same integers as search::sweep (arithmetic: search/fixed.rs).
// Build: tools/dev.sh ptx (--fmad=false keeps the f32 lerp unfused, as on the CPU).

#define WARPS 4
#define NONE (-0x7fffffffffffffffLL - 1)
// Consecutive f0 steps per lane. When (R−1)·step < 1 bin they share a 3-value window of the row.
#define R 4

// fixed::term(w · fixed::lerp) given the two row values around p.
__device__ __forceinline__ long long term(float a, float b, long long p, float w) {
    const float fr = __fmul_rn((float)(unsigned)((p >> 8) & 0xffffff), 1.0f / 16777216.0f);
    const float v = __fadd_rn(__fmul_rn(a, __fsub_rn(1.0f, fr)), __fmul_rn(b, fr));
    return (long long)__float2int_rz(__fmul_rn(__fmul_rn(w, v), 16777216.0f));
}

// Lanes per step group: the largest power of two whose padding of `groups` is ≤ 5%. The other lane bits
// split the frames; partial sums are i64, so any summation order is exact.
__device__ __forceinline__ unsigned group_width(unsigned groups) {
    for (unsigned g = 32; g > 1; g >>= 1) {
        if ((groups + g - 1) / g * g * 20 <= groups * 21) return g;
    }
    return 1;
}

// R steps from p0 (step apart) of one row, into acc. `window` ((R−1)·step < 1 bin) and all R positions
// in-row: they lie in bins j0 and j0+1, so row[j0..=j0+2] covers every lerp.
__device__ __forceinline__ void terms(
    const float* row, long long p0, long long step, long long top, float w, bool window, long long* acc)
{
    if (window && p0 >= 0 && p0 + (R - 1) * step < top) {
        const long long j0 = p0 >> 32;
        const float v0 = row[j0], v1 = row[j0 + 1];
        // j0+2 can be past the row only when every position has bin j0.
        const float v2 = ((p0 + (R - 1) * step) >> 32) > j0 ? row[j0 + 2] : 0.0f;
#pragma unroll
        for (int r = 0; r < R; ++r) {
            const long long p = p0 + r * step;
            const bool d = (p >> 32) > j0;
            acc[r] += term(d ? v1 : v0, d ? v2 : v1, p, w);
        }
    } else {
#pragma unroll
        for (int r = 0; r < R; ++r) {
            const long long p = p0 + r * step;
            if (p >= 0 && p < top) acc[r] += term(row[p >> 32], row[(p >> 32) + 1], p, w);
        }
    }
}

// One warp per (event instance, clock index k); a tile is g step groups (R steps each) × (32/g) frame phases.
extern "C" __global__ void best_f0(
    const float* __restrict__ plane, unsigned n_f,
    const long long* __restrict__ offsets,
    const unsigned long long* __restrict__ ev_off_start,
    const unsigned* __restrict__ ev_frame0,
    const unsigned* __restrict__ ev_nframes,
    const long long* __restrict__ ev_b0,
    const unsigned* __restrict__ ev_nsteps,
    const double* __restrict__ ev_norm,
    const long long* __restrict__ hs, const float* __restrict__ hw, unsigned n_h,
    long long step, unsigned n_k, unsigned long long n_pairs,
    double* __restrict__ out_z, unsigned* __restrict__ out_step, unsigned* __restrict__ out_cov)
{
    const unsigned long long pair = (unsigned long long)blockIdx.x * WARPS + threadIdx.x / 32;
    if (pair >= n_pairs) return;  // whole warps only: pairs are warp-uniform
    const unsigned lane = threadIdx.x & 31;
    const unsigned ev = (unsigned)(pair / n_k), k = (unsigned)(pair % n_k);
    const long long* offs = offsets + ev_off_start[ev] + k;
    const unsigned frame0 = ev_frame0[ev], nframes = ev_nframes[ev], nsteps = ev_nsteps[ev];
    const long long b0 = ev_b0[ev], top = (long long)(n_f - 1) << 32;
    const double norm = ev_norm[ev];
    const bool window = (R - 1) * step < (1LL << 32);

    double best = -INFINITY;
    unsigned best_step = 0xffffffffu;
    const unsigned g = group_width((nsteps + R - 1) / R), phases = 32 / g, phase = lane / g;
    for (unsigned s0 = 0; s0 < nsteps; s0 += g * R) {
        const unsigned s = s0 + (lane % g) * R;
        long long acc[R] = {};
        if (s < nsteps) {
            const long long bs = b0 + (long long)s * step;
            for (unsigned i = phase; i < nframes; i += phases) {
                const long long off = offs[i];
                if (off == NONE) continue;
                const float* row = plane + (size_t)(frame0 + i) * n_f;
                for (unsigned h = 0; h < n_h; ++h) terms(row, bs + off + hs[h], step, top, hw[h], window, acc);
            }
        }
#pragma unroll
        for (int r = 0; r < R; ++r) {
            long long a = acc[r];
            for (unsigned o = g; o < 32; o <<= 1) a += __shfl_xor_sync(0xffffffffu, a, o);
            if (phase == 0 && s + r < nsteps) {
                const double z = (double)a * (1.0 / 16777216.0) / norm;
                if (z > best) { best = z; best_step = s + r; }
            }
        }
    }
    // Warp argmax; ties keep the lowest step, as the CPU's first-strict-maximum scan does.
    for (unsigned w = 16; w > 0; w >>= 1) {
        const double oz = __shfl_down_sync(0xffffffffu, best, w);
        const unsigned os = __shfl_down_sync(0xffffffffu, best_step, w);
        if (oz > best || (oz == best && os < best_step)) { best = oz; best_step = os; }
    }
    unsigned cov = 0;
    for (unsigned i = lane; i < nframes; i += 32) cov += offs[i] != NONE;
    for (unsigned o = 16; o > 0; o >>= 1) cov += __shfl_xor_sync(0xffffffffu, cov, o);
    if (lane == 0) {
        out_z[pair] = best;
        out_step[pair] = best_step;
        out_cov[pair] = cov;
    }
}
