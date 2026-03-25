#!/usr/bin/env bash
# Benchmark seqtable against other tools for sequence counting.
#
# Prerequisites: nix develop (or cargo, hyperfine, jq in PATH)
# Fixtures: cargo run --example generate_fixtures --release -- --size <SIZE>
#
# Usage:
#   ./benches/benchmark.sh [small|medium|large]
#
# Output: benches/results/summary.tsv

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
FIXTURE_DIR="$ROOT_DIR/tests/fixtures"
RESULT_DIR="$ROOT_DIR/benches/results"
SEQTABLE="$ROOT_DIR/target/release/seqtable"
SUMMARY="$RESULT_DIR/summary.tsv"

SIZE="${1:-medium}"
WARMUP=1
RUNS=3

info() { echo -e "\033[0;36m[bench]\033[0m $*" >&2; }
err() {
  echo -e "\033[0;31m[bench]\033[0m $*" >&2
  exit 1
}

# --- Prerequisites ---

for tool in hyperfine jq; do
  command -v "$tool" &>/dev/null || err "$tool not found"
done

# Build release binary
info "Building seqtable (release)..."
cargo build --release --quiet || err "Failed to build. Run from nix develop."

# --- Fixture selection ---

declare -a FILES
case "$SIZE" in
small) FILES=("$FIXTURE_DIR"/sm_*.fastq) ;;
medium) FILES=("$FIXTURE_DIR"/md_*.fq.gz) ;;
large) FILES=("$FIXTURE_DIR"/lg_*.fq.gz) ;;
*) err "Unknown size: $SIZE. Use small|medium|large" ;;
esac

if [ ${#FILES[@]} -eq 0 ] || [ ! -f "${FILES[0]}" ]; then
  err "No fixtures for size=$SIZE. Run: cargo run --example generate_fixtures --release -- --size $SIZE"
fi

mkdir -p "$RESULT_DIR"
BENCH_TMPDIR=$(mktemp -d)
trap 'rm -rf "$BENCH_TMPDIR"' EXIT

# --- Summary header ---

printf "tool\tthreads\tfile\tmean_s\tstddev_s\tpeak_rss_mb\n" >"$SUMMARY"

# --- Measurement helpers ---

measure_rss_mb() {
  # Run a command, return peak RSS in MB
  local time_out
  time_out=$(/usr/bin/time -l bash -c "$1" 2>&1 >/dev/null) || true
  local rss_bytes
  rss_bytes=$(echo "$time_out" | grep -i "maximum resident" | awk '{print $1}')
  if [ -z "$rss_bytes" ]; then
    echo "0"
    return
  fi
  if [ "$(uname)" = "Darwin" ]; then
    echo "scale=1; $rss_bytes / 1048576" | bc
  else
    echo "scale=1; $rss_bytes / 1024" | bc
  fi
}

bench() {
  local tool="$1" threads="$2" file="$3" cmd="$4"
  local fname label json mean stddev rss
  fname=$(basename "$file")
  label="${tool}_${threads}t_${fname}"
  json="$BENCH_TMPDIR/${label}.json"

  info "  $tool (${threads}t) on $fname"

  # Time measurement with hyperfine
  hyperfine \
    --warmup "$WARMUP" \
    --runs "$RUNS" \
    --export-json "$json" \
    --command-name "$label" \
    -- "$cmd"

  mean=$(jq -r '.results[0].mean' "$json")
  stddev=$(jq -r '.results[0].stddev' "$json")

  # Peak RSS (single run)
  rss=$(measure_rss_mb "$cmd")

  printf "%s\t%s\t%s\t%.3f\t%.3f\t%s\n" "$tool" "$threads" "$fname" "$mean" "$stddev" "$rss" >>"$SUMMARY"
}

# --- Run benchmarks ---

info "Benchmarking size=$SIZE (${#FILES[@]} files, $RUNS runs each)"
echo

for file in "${FILES[@]}"; do
  fname=$(basename "$file")
  info "=== $fname ==="

  bench "seqtable" "1" "$file" \
    "$SEQTABLE $file -o $BENCH_TMPDIR -f csv -q -t 1"

  bench "seqtable" "auto" "$file" \
    "$SEQTABLE $file -o $BENCH_TMPDIR -f csv -q"

  if command -v seqkit &>/dev/null; then
    bench "seqkit" "1" "$file" \
      "seqkit fx2tab -j 1 $file | cut -f2 | sort | uniq -c | sort -rn > $BENCH_TMPDIR/out.txt"

    bench "seqkit" "auto" "$file" \
      "seqkit fx2tab $file | cut -f2 | sort | uniq -c | sort -rn > $BENCH_TMPDIR/out.txt"
  fi

  cat_cmd="cat"
  [[ $file == *.gz ]] && cat_cmd="gzip -dc"
  bench "awk" "1" "$file" \
    "$cat_cmd $file | awk 'NR%4==2{a[\$0]++}END{for(k in a)print a[k],k}' | sort -rn > $BENCH_TMPDIR/out.txt"

  echo
done

# --- Print summary ---

info "Results:"
echo
column -t -s $'\t' "$SUMMARY"
echo
info "Raw TSV: $SUMMARY"

cp "$BENCH_TMPDIR"/*.json "$RESULT_DIR/" 2>/dev/null || true
