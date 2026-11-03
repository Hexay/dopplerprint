// f0 sweep on the GPU; same integers as search::sweep (arithmetic: search/fixed.rs).
// Build: tools/dev.sh ptx (--fmad=false keeps the f32 lerp unfused, as on the CPU).

#define WARPS 4
#define NONE (-0x7fffffffffffffffLL - 1)

// fixed::lerp then fixed::term of w·lerp.
__device__ __forceinline__ long long term(const float* row, long long p, float w) {
    const long long j = p >> 32;
    const float fr = __fmul_rn((float)(unsigned)((p >> 8) & 0xffffff), 1.0f / 16777216.0f);
    const float v = __fadd_rn(__fmul_rn(row[j], __fsub_rn(1.0f, fr)), __fmul_rn(row[j + 1], fr));
    return (long long)__float2int_rz(__fmul_rn(__fmul_rn(w, v), 16777216.0f));
}

// Steps per 32-lane tile: the largest power of two whose padding of nsteps is ≤ 5%. The other lane bits
// split the frames; partial sums are i64, so any summation order is exact.
__device__ __forceinline__ unsigned group_width(unsigned nsteps) {
    for (unsigned g = 32; g > 1; g >>= 1) {
        if ((nsteps + g - 1) / g * g * 20 <= nsteps * 21) return g;
    }
    return 1;
}

// One warp per (event instance, clock index k); a tile is g f0 steps × (32/g) frame phases.
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
    double* __restrict__ out_z, unsigned* __restrict__ out_step)
{
    const unsigned long long pair = (unsigned long long)blockIdx.x * WARPS + threadIdx.x / 32;
    if (pair >= n_pairs) return;  // whole warps only: pairs are warp-uniform
    const unsigned lane = threadIdx.x & 31;
    const unsigned ev = (unsigned)(pair / n_k), k = (unsigned)(pair % n_k);
    const long long* offs = offsets + ev_off_start[ev] + k;
    const unsigned frame0 = ev_frame0[ev], nframes = ev_nframes[ev], nsteps = ev_nsteps[ev];
    const long long b0 = ev_b0[ev], top = (long long)(n_f - 1) << 32;
    const double norm = ev_norm[ev];

    double best = -INFINITY;
    unsigned best_step = 0xffffffffu;
    const unsigned g = group_width(nsteps), phases = 32 / g, phase = lane / g;
    for (unsigned s0 = 0; s0 < nsteps; s0 += g) {
        const unsigned s = s0 + lane % g;
        long long acc = 0;
        if (s < nsteps) {
            const long long bs = b0 + (long long)s * step;
            for (unsigned i = phase; i < nframes; i += phases) {
                const long long off = offs[i];
                if (off == NONE) continue;
                const float* row = plane + (size_t)(frame0 + i) * n_f;
                const long long base = bs + off;
                for (unsigned h = 0; h < n_h; ++h) {
                    const long long p = base + hs[h];
                    if (p >= 0 && p < top) acc += term(row, p, hw[h]);
                }
            }
        }
        for (unsigned o = g; o < 32; o <<= 1) acc += __shfl_xor_sync(0xffffffffu, acc, o);
        if (phase == 0 && s < nsteps) {
            const double z = (double)acc * (1.0 / 16777216.0) / norm;
            if (z > best) { best = z; best_step = s; }
        }
    }
    // Warp argmax; ties keep the lowest step, as the CPU's first-strict-maximum scan does.
    for (unsigned w = 16; w > 0; w >>= 1) {
        const double oz = __shfl_down_sync(0xffffffffu, best, w);
        const unsigned os = __shfl_down_sync(0xffffffffu, best_step, w);
        if (oz > best || (oz == best && os < best_step)) { best = oz; best_step = os; }
    }
    if (lane == 0) {
        out_z[pair] = best;
        out_step[pair] = best_step;
    }
}
