#!/usr/bin/env bash
# Usage: variants.sh OUTPUT [PASSES=7]
# Builds the bench once, then runs PASSES passes of `bench --variants` pinned to
# CPU ${BENCH_CPU:-3}, writing OUTPUT/pass-<k>.csv.
set -euo pipefail

out=${1:?usage: variants.sh OUTPUT [PASSES=7]}
passes=${2:-7}
cpu=${BENCH_CPU:-3}
mkdir -p "$out"
out=$(cd "$out" && pwd)
# Cargo reads the toolchain and target-cpu=native from the working directory.
cd "$(dirname "$0")/.."

bin=$(cargo build --release --example bench --no-default-features --features avx2 \
    --message-format=json-render-diagnostics |
    sed -n 's/.*"executable":"\([^"]*\)".*/\1/p')

for k in $(seq 1 "$passes"); do
    echo "$(date -u +%FT%TZ) pass-$k $(cut -d" " -f1-3 /proc/loadavg)" | tee -a "$out/load.txt" >&2
    taskset -c "$cpu" "$bin" --variants --min 10 --max 22 --step 2 >"$out/pass-$k.csv"
done
