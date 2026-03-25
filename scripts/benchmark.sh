#!/usr/bin/env bash
# Benchmark seqtable against other tools for sequence counting.
#
# Run via: nix run .#benchmark -- [small|medium|large|all]
# Fixtures: cargo run --example generate_fixtures --release -- --size <SIZE>
#
# Output: benches/results/summary_<size>_<timestamp>.tsv

FIXTURE_DIR="${FIXTURE_DIR:-tests/fixtures}"
RESULT_DIR="${RESULT_DIR:-benches/results}"

SIZE="${1:-medium}"
WARMUP=3
RUNS=5

info() { echo -e "\033[0;36m[bench]\033[0m $*" >&2; }
err() {
  echo -e "\033[0;31m[bench]\033[0m $*" >&2
  exit 1
}

run_size() {
  local size="$1"

  # Fixture selection
  declare -a FILES
  case "$size" in
  small) FILES=("$FIXTURE_DIR"/sm_*.fastq) ;;
  medium) FILES=("$FIXTURE_DIR"/md_*.fq.gz) ;;
  large) FILES=("$FIXTURE_DIR"/lg_*.fq.gz) ;;
  *) err "Unknown size: $size" ;;
  esac

  if [ "${#FILES[@]}" -eq 0 ] || [ ! -f "${FILES[0]}" ]; then
    info "No fixtures for size=$size, skipping. Generate: cargo run --example generate_fixtures --release -- --size $size"
    return
  fi

  # Timestamped output file (no overwrite)
  local timestamp
  timestamp=$(date +%Y%m%d_%H%M%S)
  local summary="$RESULT_DIR/summary_${size}_${timestamp}.tsv"

  printf "tool\tthreads\tfile\tmean_s\tstddev_s\tpeak_rss_mb\n" >"$summary"

  info "Benchmarking size=$size (${#FILES[@]} files, warmup=$WARMUP, runs=$RUNS)"
  echo

  for file in "${FILES[@]}"; do
    local fname
    fname=$(basename "$file")
    info "=== $fname ==="

    # seqtable
    bench "seqtable" "1" "$file" \
      "seqtable $file -o $BENCH_TMPDIR -f csv -q -t 1" "$summary"

    bench "seqtable" "auto" "$file" \
      "seqtable $file -o $BENCH_TMPDIR -f csv -q" "$summary"

    # seqkit
    bench "seqkit" "1" "$file" \
      "seqkit fx2tab -j 1 $file | cut -f2 | sort | uniq -c | sort -rn > $BENCH_TMPDIR/out.txt" "$summary"
    bench "seqkit" "auto" "$file" \
      "seqkit fx2tab $file | cut -f2 | sort | uniq -c | sort -rn > $BENCH_TMPDIR/out.txt" "$summary"

    # awk (single-threaded)
    local cat_cmd="cat"
    [[ $file == *.gz ]] && cat_cmd="gzip -dc"
    bench "awk" "1" "$file" \
      "$cat_cmd $file | awk 'NR%4==2{a[\$0]++}END{for(k in a)print a[k],k}' | sort -rn > $BENCH_TMPDIR/out.txt" "$summary"

    # awk + GNU parallel
    bench "awk+parallel" "auto" "$file" \
      "$cat_cmd $file | parallel --pipe -k --block 50M 'awk \"NR%4==2{a[\$0]++}END{for(k in a)print a[k],k}\"' | awk '{a[\$2]+=\$1}END{for(k in a)print a[k],k}' | sort -rn > $BENCH_TMPDIR/out.txt" "$summary"

    echo
  done

  info "Results ($size):"
  echo
  column -t -s $'\t' "$summary"
  echo

  # Best performers (skip header, sort by mean_s or peak_rss_mb)
  local fastest fastest_rss
  fastest=$(tail -n +2 "$summary" | sort -t$'\t' -k4 -n | head -1)
  fastest_rss=$(tail -n +2 "$summary" | sort -t$'\t' -k6 -n | head -1)

  info "Fastest (wall time): $(echo "$fastest" | awk -F'\t' '{printf "%s/%s on %s: %.3fs", $1, $2, $3, $4}')"
  info "Lowest memory (RSS): $(echo "$fastest_rss" | awk -F'\t' '{printf "%s/%s on %s: %sMB", $1, $2, $3, $6}')"
  echo
  info "Saved: $summary"
}

bench() {
  local tool="$1" threads="$2" file="$3" cmd="$4" summary="$5"
  local fname label json mean stddev rss
  fname=$(basename "$file")
  label="${tool}_${threads}t_${fname}"
  json="$BENCH_TMPDIR/${label}.json"

  info "  $tool (${threads}t) on $fname"

  # --prepare: drop filesystem caches (best-effort, needs sudo on Linux)
  # On macOS no easy way, rely on warmup instead
  hyperfine \
    --warmup "$WARMUP" \
    --runs "$RUNS" \
    --prepare "sync" \
    --export-json "$json" \
    --command-name "$label" \
    -- "$cmd"

  mean=$(jq -r '.results[0].mean' "$json")
  stddev=$(jq -r '.results[0].stddev' "$json")
  rss=$(measure_rss_mb "$cmd")

  printf "%s\t%s\t%s\t%.3f\t%.3f\t%s\n" \
    "$tool" "$threads" "$fname" "$mean" "$stddev" "$rss" >>"$summary"
}

measure_rss_mb() {
  local time_out rss_bytes
  time_out=$(/usr/bin/time -l bash -c "$1" 2>&1 >/dev/null) || true
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

# --- Main ---

mkdir -p "$RESULT_DIR"
BENCH_TMPDIR=$(mktemp -d)
trap 'rm -rf "$BENCH_TMPDIR"' EXIT

if [ "$SIZE" = "all" ]; then
  for s in small medium large; do
    run_size "$s"
  done
else
  run_size "$SIZE"
fi

# Copy JSON details
cp "$BENCH_TMPDIR"/*.json "$RESULT_DIR/" 2>/dev/null || true
info "Done! All results in $RESULT_DIR/"
