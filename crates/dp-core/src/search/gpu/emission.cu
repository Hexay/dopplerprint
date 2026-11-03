// Doppler offsets on the GPU: the retarded-time Newton solve of physics::emission (uniform medium) and
// fixed::pos(bpo·log2 F). Same equations as the CPU, not bit-identical to it (CUDA vs libm sqrt/log2
// rounding); see gpu/emit.rs. Build: tools/dev.sh ptx.

#define NONE (-0x7fffffffffffffffLL - 1)
#define MAX_ITERS 30
#define CHUNK 16
#define MAX_WALK 4

struct V { double x, y, z; };
__device__ __forceinline__ V vadd(V a, V b) { return {a.x + b.x, a.y + b.y, a.z + b.z}; }
__device__ __forceinline__ V vsub(V a, V b) { return {a.x - b.x, a.y - b.y, a.z - b.z}; }
__device__ __forceinline__ V vmul(V a, double s) { return {a.x * s, a.y * s, a.z * s}; }
__device__ __forceinline__ double vdot(V a, V b) { return a.x * b.x + a.y * b.y + a.z * b.z; }
__device__ __forceinline__ V ld(const double* p, unsigned i) { return {p[3 * i], p[3 * i + 1], p[3 * i + 2]}; }

struct Track { const double* t; const double* pos; const double* vel; unsigned n; };

// track::state_at_from: hinted partition point (≤ MAX_WALK steps, else binary search), then Hermite.
__device__ bool state_at(const Track& tr, double t, unsigned& hint, V& pos, V& vel) {
    if (tr.n == 0 || !(t >= tr.t[0] && t <= tr.t[tr.n - 1])) return false;
    if (tr.n == 1) { pos = ld(tr.pos, 0); vel = ld(tr.vel, 0); return true; }
    unsigned pp = min(hint, tr.n), walked = 0;
    while (pp < tr.n && tr.t[pp] <= t && walked < MAX_WALK) { ++pp; ++walked; }
    while (pp > 0 && tr.t[pp - 1] > t && walked < MAX_WALK) { --pp; ++walked; }
    if (!((pp == tr.n || tr.t[pp] > t) && (pp == 0 || tr.t[pp - 1] <= t))) {
        unsigned lo = 0, hi = tr.n;
        while (lo < hi) { unsigned m = (lo + hi) / 2; if (tr.t[m] <= t) lo = m + 1; else hi = m; }
        pp = lo;
    }
    hint = pp;
    const unsigned i = min(max(pp, 1u), tr.n - 1);
    const double ta = tr.t[i - 1], h = tr.t[i] - ta, s = (t - ta) / h, s2 = s * s, s3 = s2 * s;
    const V ap = ld(tr.pos, i - 1), av = ld(tr.vel, i - 1), bp = ld(tr.pos, i), bv = ld(tr.vel, i);
    pos = vadd(vadd(vmul(ap, 2.0 * s3 - 3.0 * s2 + 1.0), vmul(av, (s3 - 2.0 * s2 + s) * h)),
               vadd(vmul(bp, 3.0 * s2 - 2.0 * s3), vmul(bv, (s3 - s2) * h)));
    vel = vadd(vadd(vmul(vsub(bp, ap), (6.0 * s - 6.0 * s2) / h), vmul(av, 3.0 * s2 - 4.0 * s + 1.0)),
               vmul(bv, 3.0 * s2 - 2.0 * s));
    return true;
}

// physics::emission::emission_from in a uniform medium; returns the Doppler factor, tau via `tau_io`.
__device__ bool solve(const Track& tr, V obs, double t, double c, V w, double& tau_io, unsigned& hint, double& factor) {
    const double lo = tr.t[0], hi = tr.t[tr.n - 1];
    const double tol = fmax(1e-9, fabs(t) * 4.0 * 2.220446049250313e-16);
    double tau = fmin(fmax(tau_io, lo), hi);
    for (int it = 0; it <= MAX_ITERS; ++it) {
        V p, v;
        if (!state_at(tr, tau, hint, p, v)) return false;
        const V d = vsub(vsub(obs, p), vmul(w, t - tau));
        const double r = sqrt(vdot(d, d));
        const V k = vmul(d, 1.0 / r);
        const double g = (tau - t) + r / c, dg = 1.0 + vdot(k, vsub(w, v)) / c;
        if (fabs(g) < tol) {
            const double c_eff = c + vdot(w, k);
            factor = c_eff / (c_eff - vdot(v, k));
            tau_io = tau;
            return true;
        }
        if ((tau <= lo && g > 0.0) || (tau >= hi && g < 0.0)) return false;
        tau = fmin(fmax(tau - g / dg, lo), hi);
    }
    return false;
}

// One thread per (instance, chunk of CHUNK frames); warm starts within the chunk as physics::emissions.
extern "C" __global__ void offsets(
    const double* __restrict__ trk_t, const double* __restrict__ trk_pos, const double* __restrict__ trk_vel,
    const unsigned* __restrict__ trk_start, const unsigned* __restrict__ trk_len,
    const unsigned* __restrict__ in_track, const double* __restrict__ in_obs, const long long* __restrict__ in_first,
    const double* __restrict__ in_clock, const unsigned* __restrict__ in_n, const double* __restrict__ in_c,
    const double* __restrict__ in_wind, const unsigned long long* __restrict__ in_off_start,
    const unsigned* __restrict__ ch_inst, const unsigned* __restrict__ ch_j0, unsigned n_chunks,
    double spec_t0, double dt, double bpo, long long* __restrict__ out)
{
    const unsigned ch = blockIdx.x * blockDim.x + threadIdx.x;
    if (ch >= n_chunks) return;
    const unsigned in = ch_inst[ch], j0 = ch_j0[ch], j1 = min(j0 + CHUNK, in_n[in]);
    const unsigned ti = in_track[in], s0 = trk_start[ti];
    const Track tr = {trk_t + s0, trk_pos + 3 * (size_t)s0, trk_vel + 3 * (size_t)s0, trk_len[ti]};
    const V obs = ld(in_obs, in), w = ld(in_wind, in);
    const double c = in_c[in], clock = in_clock[in];
    const long long first = in_first[in];
    long long* o = out + in_off_start[in];
    unsigned hint = 0;
    int have = 0;  // solutions available for the predictor
    double t1 = 0, tau1 = 0, f1 = 0, t0 = 0, tau0 = 0;
    for (unsigned j = j0; j < j1; ++j) {
        const double t = spec_t0 + (double)(first + (long long)j) * dt + clock;
        double guess;
        if (have >= 2 && t0 != t1) {
            const double h0 = t0 - t1, cc = (tau0 - tau1 - f1 * h0) / (h0 * h0), h = t - t1;
            guess = tau1 + h * (f1 + cc * h);
        } else if (have == 1) {
            guess = tau1 + (t - t1) * f1;
        } else {
            V p, v;
            if (tr.n == 0 || !state_at(tr, fmin(fmax(t, tr.t[0]), tr.t[tr.n - 1]), hint, p, v)) { o[j] = NONE; continue; }
            const V d = vsub(obs, p);
            guess = t - sqrt(vdot(d, d)) / c;
        }
        double tau = guess, f;
        if (!solve(tr, obs, t, c, w, tau, hint, f)) { o[j] = NONE; have = 0; continue; }
        const double x = bpo * log2(f);
        o[j] = isfinite(x) ? (long long)round(x * 4294967296.0) : NONE;
        t0 = t1; tau0 = tau1; t1 = t; tau1 = tau; f1 = f;
        have = have < 2 ? have + 1 : 2;
    }
}
