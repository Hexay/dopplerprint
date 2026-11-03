#!/usr/bin/env bash
# CPU↔CUDA equivalence on the GPU box, plus drift vs references from the f64-scoring code in /root/ref
# (search_dump.json: full ranking of two_event_search_recovers_observer; parity_case.json: oysterbay parity).
# PASS/FAIL sweep = CPU vs CUDA on host offsets (DP_CUDA_HOST_OFFSETS=1), must be bit-identical.
# PASS/FAIL e2e   = CPU vs CUDA with device offsets: same ranking keys, scores within TOL relative.
# DRIFT           = vs the f64 reference. Usage: tools/gpu_parity.sh   (syncs first).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
"$ROOT/tools/gpu.sh" sync
"$ROOT/tools/gpu.sh" run bash -s <<'EOF'
set -euo pipefail
cargo build -q --release -p dp-cli 2>&1 | tail -n 20
T='test(two_event_search_recovers_observer)'
dump() {  # name, then env assignments
  local name=$1; shift
  env "$@" DP_SEARCH_DUMP=/root/ref/dump_$name.json cargo nextest run --release -p dp-core --cargo-quiet \
    --run-ignored all --no-capture -E "$T" 2>&1 | grep -E "scoring on|PASS|FAIL" | head -n 3
}
dump cpu DP_BACKEND=cpu
dump cudahost DP_BACKEND=cuda DP_CUDA_HOST_OFFSETS=1
dump cuda DP_BACKEND=cuda
cmp -s /root/ref/dump_cpu.json /root/ref/dump_cudahost.json && echo "PASS sweep dump cpu == cuda(host offsets)" \
  || echo "FAIL sweep dump cpu != cuda(host offsets)"
EXP=experiments/oysterbay_dg_parity.toml
sed 's/^\[search\]$/[search]\nbackend = "cpu"/' $EXP > /root/ref/parity_cpu.toml
for exp in $EXP /root/ref/parity_cpu.toml; do
  target/release/dopplerprint eval --experiment $exp > /dev/null 2>&1
  cp runs/$(ls -t runs | head -1)/oysterbay_20260908_colsto.json /root/ref/case_$(basename $exp .toml).json
done
python3 -I - <<'PY'
import json
L = lambda p: json.load(open(p))
TOL = 1e-6
key = lambda c: (tuple(c["cell"].values()), tuple(c["track_ids"]))
rel = lambda a, b: abs(a - b) / max(1.0, abs(a), abs(b))

def close(a, b, path=""):
    """Structural equality with floats within TOL; returns the first differing path or None."""
    if isinstance(a, float) or isinstance(b, float):
        return None if isinstance(a, (int, float)) and isinstance(b, (int, float)) and rel(a, b) <= TOL else path
    if isinstance(a, dict) and isinstance(b, dict):
        if a.keys() != b.keys(): return path + "{keys}"
        return next((p for k in a if (p := close(a[k], b[k], f"{path}.{k}"))), None)
    if isinstance(a, list) and isinstance(b, list):
        if len(a) != len(b): return path + "[len]"
        return next((p for i, (x, y) in enumerate(zip(a, b)) if (p := close(x, y, f"{path}[{i}]"))), None)
    return None if a == b else path

cpu, cuda = L("/root/ref/dump_cpu.json")["ranked"], L("/root/ref/dump_cuda.json")["ranked"]
n = min(200, len(cpu))
same = sum(key(a) == key(b) for a, b in zip(cpu[:n], cuda[:n]))
dmax = max((rel(a["score"], b["score"]) for a, b in zip(cpu[:n], cuda[:n]) if key(a) == key(b)), default=0.0)
ok = same == n and dmax <= TOL and len(cpu) == len(cuda)
print(f"{'PASS' if ok else 'FAIL'} e2e dump top-{n}: {same}/{n} same cells+flights, max rel score diff {dmax:.2e}, sizes {len(cpu)} vs {len(cuda)}")

strip = lambda d: {k: v for k, v in d.items() if k not in ("runtime_s", "device", "timing")}
gpu, cpuc, ref = L("/root/ref/case_oysterbay_dg_parity.json"), L("/root/ref/case_parity_cpu.json"), L("/root/ref/parity_case.json")
diff = close(strip(cpuc), strip(gpu))
print(("PASS" if diff is None else f"FAIL (first diff at {diff})") + f" e2e parity case; runtime cuda {gpu['runtime_s']:.2f} s, cpu {cpuc['runtime_s']:.2f} s")
for k in ("error_m", "flights_correct", "truth_tuple_rank", "margin", "clock_err_s"):
    print(f"DRIFT parity {k}: ref {ref[k]} now {gpu[k]}")
old = L("/root/ref/search_dump.json")["ranked"]
n = 50
same = sum(key(a) == key(b) for a, b in zip(cpu[:n], old[:n]))
dmax = max(abs(a["score"] - b["score"]) for a, b in zip(cpu[:n], old[:n]) if key(a) == key(b))
print(f"DRIFT dump top-{n}: {same}/{n} identical cells+flights, max |score diff| {dmax:.2e}, sizes {len(cpu)} vs {len(old)}")
PY
EOF
