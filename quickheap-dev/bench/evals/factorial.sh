#!/usr/bin/env bash
set -euo pipefail

out=${1:?usage: factorial.sh OUTPUT OPTS [PASSES=5]}
opts=${2:?usage: factorial.sh OUTPUT OPTS [PASSES=5]}
passes=${3:-5}
cpu=${BENCH_CPU:-3}
mkdir -p "$out"
out=$(cd "$out" && pwd)
cd "$(dirname "$0")/.."

bin=$(cargo build --release --example bench --no-default-features --features avx2 \
    --message-format=json-render-diagnostics |
    sed -n 's/.*"executable":"\([^"]*\)".*/\1/p')

for k in $(seq 1 "$passes"); do
    order=$(python3 -c 'import random, sys; x = sys.argv[2].split(","); random.Random(int(sys.argv[1])).shuffle(x); print(",".join(x))' "$k" "$opts")
    echo "$(date -u +%FT%TZ) pass-$k $(cut -d" " -f1-3 /proc/loadavg) $order" | tee -a "$out/load.txt" >&2
    taskset -c "$cpu" "$bin" --variants --opts "$order" --min 10 --max 22 --step 2 >"$out/pass-$k.csv"
done
