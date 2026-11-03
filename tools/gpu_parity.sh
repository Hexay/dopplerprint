#!/usr/bin/env bash
# CPU↔CUDA equivalence on the GPU box, plus drift vs references from the f64-scoring code in /root/ref
# (search_dump.json: full ranking of two_event_search_recovers_observer; parity_case.json: oysterbay parity).
# Usage: tools/gpu_parity.sh   (syncs first). PASS/FAIL = CPU vs CUDA bit-identity; DRIFT = vs f64 reference.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
"$ROOT/tools/gpu.sh" sync
"$ROOT/tools/gpu.sh" run bash -s <<'EOF'
set -euo pipefail
cargo build -q --release -p dp-cli 2>&1 | tail -n 20
T='test(two_event_search_recovers_observer)'
for b in cpu cuda; do
  DP_BACKEND=$b DP_SEARCH_DUMP=/root/ref/dump_$b.json cargo nextest run --release -p dp-core --cargo-quiet \
    --run-ignored all --no-capture -E "$T" 2>&1 | grep -E "scoring on|PASS|FAIL" | head -n 3
done
cmp -s /root/ref/dump_cpu.json /root/ref/dump_cuda.json && echo "PASS dump cpu == cuda" || echo "FAIL dump cpu != cuda"
EXP=experiments/oysterbay_dg_parity.toml
sed 's/^\[search\]$/[search]\nbackend = "cpu"/' $EXP > /root/ref/parity_cpu.toml
for exp in $EXP /root/ref/parity_cpu.toml; do
  target/release/dopplerprint eval --experiment $exp > /dev/null 2>&1
  cp runs/$(ls -t runs | head -1)/oysterbay_20260908_colsto.json /root/ref/case_$(basename $exp .toml).json
done
python3 -I - <<'PY'
import json
L = lambda p: json.load(open(p))
strip = lambda d: {k: v for k, v in d.items() if k not in ("runtime_s", "device", "timing")}
gpu, cpu, ref = L("/root/ref/case_oysterbay_dg_parity.json"), L("/root/ref/case_parity_cpu.json"), L("/root/ref/parity_case.json")
print(("PASS" if strip(gpu) == strip(cpu) else "FAIL") + f" parity case cpu == cuda; runtime cuda {gpu['runtime_s']:.2f} s, cpu {cpu['runtime_s']:.2f} s")
for k in ("error_m", "flights_correct", "truth_tuple_rank", "margin", "clock_err_s"):
    print(f"DRIFT parity {k}: ref {ref[k]} now {gpu[k]}")
new, old = L("/root/ref/dump_cpu.json")["ranked"], L("/root/ref/search_dump.json")["ranked"]
n = 50
key = lambda c: (tuple(c["cell"].values()), tuple(c["track_ids"]))
same = sum(key(a) == key(b) for a, b in zip(new[:n], old[:n]))
dmax = max(abs(a["score"] - b["score"]) for a, b in zip(new[:n], old[:n]) if key(a) == key(b))
print(f"DRIFT dump top-{n}: {same}/{n} identical cells+flights, max |score diff| {dmax:.2e}, sizes {len(new)} vs {len(old)}")
PY
EOF
