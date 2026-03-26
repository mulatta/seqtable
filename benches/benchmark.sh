#!/usr/bin/env bash
# Fair benchmark: seqtable vs seqkit vs awk vs awk+parallel
#
# Fairness principles:
# - All tools write output to file (not /dev/null) to include I/O cost
# - All results verified for correctness after first run
# - awk+parallel uses FASTQ-aware record splitting (--recstart '@')
# - Each tool uses its realistic usage pattern
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
warn() { echo -e "\033[0;33m[bench]\033[0m $*" >&2; }
err() {
  echo -e "\033[0;31m[bench]\033[0m $*" >&2
  exit 1
}

# --- Correctness verification ---

# Generate ground truth with plain awk (simplest, most trusted), then compare all tools
generate_reference() {
  local file="$1"
  local ref_file
  ref_file="$BENCH_TMPDIR/ref_$(basename "$file").txt"

  if [ -f "$ref_file" ]; then
    return
  fi

  info "  generating ground truth (awk)..."
  local cat_cmd="cat"
  [[ $file == *.gz ]] && cat_cmd="gzip -dc"
  $cat_cmd "$file" | awk 'NR%4==2{a[$0]++}END{for(k in a)print a[k],k}' | sort -rn >"$ref_file"
}

verify_output() {
  local tool="$1" file="$2" outfile="$3"
  local ref_file
  ref_file="$BENCH_TMPDIR/ref_$(basename "$file").txt"

  if [ ! -f "$outfile" ]; then
    warn "  $tool: no output file"
    return
  fi

  # Normalize to "count seq" format, sorted descending
  local normalized
  normalized="$BENCH_TMPDIR/normalized_${tool}_$(basename "$file").txt"
  case "$tool" in
  seqtable)
    tail -n +2 "$outfile" | awk -F, '{print $2, $1}' | sort -rn >"$normalized"
    ;;
  *)
    awk 'NF{$1=$1; print}' "$outfile" | sort -rn >"$normalized"
    ;;
  esac

  # Full diff against ground truth
  local ref_lines tool_lines
  ref_lines=$(wc -l <"$ref_file" | tr -d ' ')
  tool_lines=$(wc -l <"$normalized" | tr -d ' ')

  if [ "$ref_lines" != "$tool_lines" ]; then
    warn "  $tool: FAIL line count (expected $ref_lines, got $tool_lines)"
  elif ! diff -q "$ref_file" "$normalized" >/dev/null 2>&1; then
    local diff_lines
    diff_lines=$(diff "$ref_file" "$normalized" | grep -c "^[<>]" || true)
    warn "  $tool: FAIL $diff_lines lines differ from ground truth"
  else
    info "  $tool: OK ($ref_lines sequences)"
  fi
}

run_size() {
  local size="$1"

  declare -a FILES
  case "$size" in
  small) FILES=("$FIXTURE_DIR"/sm_*.fastq) ;;
  medium) FILES=("$FIXTURE_DIR"/md_*.fq.gz) ;;
  large) FILES=("$FIXTURE_DIR"/lg_*.fq.gz) ;;
  *) err "Unknown size: $size" ;;
  esac

  if [ "${#FILES[@]}" -eq 0 ] || [ ! -f "${FILES[0]}" ]; then
    info "No fixtures for size=$size, skipping."
    info "Generate: cargo run --example generate_fixtures --release -- --size $size"
    return
  fi

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

    local cat_cmd="cat"
    [[ $file == *.gz ]] && cat_cmd="gzip -dc"

    local out_seqkit out_awk
    out_seqkit="$BENCH_TMPDIR/out_seqkit_${fname}"
    out_awk="$BENCH_TMPDIR/out_awk_${fname}"

    # --- seqtable (1t, 4t, auto) ---
    bench "seqtable" "1" "$file" \
      "seqtable $file -o $BENCH_TMPDIR -f csv -q -t 1" "$summary"

    bench "seqtable" "4" "$file" \
      "seqtable $file -o $BENCH_TMPDIR -f csv -q -t 4" "$summary"

    bench "seqtable" "auto" "$file" \
      "seqtable $file -o $BENCH_TMPDIR -f csv -q" "$summary"

    # --- seqkit (1t, 4t, auto) ---
    bench "seqkit" "1" "$file" \
      "seqkit fx2tab -j 1 $file | cut -f2 | sort | uniq -c | sort -rn > $out_seqkit" "$summary"

    bench "seqkit" "4" "$file" \
      "seqkit fx2tab -j 4 $file | cut -f2 | sort | uniq -c | sort -rn > $out_seqkit" "$summary"

    bench "seqkit" "auto" "$file" \
      "seqkit fx2tab $file | cut -f2 | sort | uniq -c | sort -rn > $out_seqkit" "$summary"

    # --- awk (HashMap counting, single-threaded) ---
    bench "awk" "1" "$file" \
      "$cat_cmd $file | awk 'NR%4==2{a[\$0]++}END{for(k in a)print a[k],k}' | sort -rn > $out_awk" "$summary"

    # --- coreutils baseline (sort|uniq -c, single-threaded) ---
    local out_core="$BENCH_TMPDIR/out_core_${fname}"
    bench "coreutils" "1" "$file" \
      "$cat_cmd $file | awk 'NR%4==2' | sort | uniq -c | sort -rn > $out_core" "$summary"

    # --- coreutils with parallel sort ---
    bench "coreutils" "4" "$file" \
      "$cat_cmd $file | awk 'NR%4==2' | sort --parallel=4 | uniq -c | sort -rn > $out_core" "$summary"

    # --- Verify correctness against awk ground truth ---
    info "  Verifying correctness..."
    generate_reference "$file"

    local seqtable_csv="$BENCH_TMPDIR/${fname%.gz}"
    seqtable_csv="${seqtable_csv%.fastq}"
    seqtable_csv="${seqtable_csv%.fq}.csv"
    verify_output "seqtable" "$file" "$seqtable_csv"
    verify_output "seqkit" "$file" "$out_seqkit"
    verify_output "coreutils" "$file" "$out_core"

    echo
  done

  # --- Results ---

  info "Results ($size):"
  echo
  column -t -s $'\t' "$summary"
  echo

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

cp "$BENCH_TMPDIR"/*.json "$RESULT_DIR/" 2>/dev/null || true
info "Done! All results in $RESULT_DIR/"
