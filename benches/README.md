# Benchmarks

## Quick Start

```bash
# Generate fixtures (one-time)
cargo run --example generate_fixtures --release -- --size medium

# Option 1: nix run (slow first time due to flake eval, ~1min)
nix run .#benchmark -- medium

# Option 2: build once, run fast (recommended for repeated runs)
nix build .#benchmark-script -o result-bench
./result-bench/bin/seqtable-benchmark medium
```

## Fairness Principles

1. **All tools write to file** — not `/dev/null`. I/O cost is included equally.
2. **Correctness verified** — each tool's output is compared against seqtable's reference. Mismatches are flagged.
3. **FASTQ-aware splitting** — `awk+parallel` uses `--recstart '@'` to avoid splitting mid-record.
4. **Fixed thread counts** — 1t/4t/auto for reproducible scaling comparison, not just "auto" which varies by machine.
5. **Realistic usage** — each tool uses its natural invocation pattern (seqtable writes CSV, seqkit pipes through sort|uniq -c, awk uses associative arrays).
6. **Statistical rigor** — warmup=3, runs=5, `sync` between runs. Mean + stddev reported.

## What We Measure

| Metric          | Tool                 | Notes                                          |
| --------------- | -------------------- | ---------------------------------------------- |
| Wall time       | hyperfine            | Includes warmup, statistical outlier detection |
| Peak RSS        | `/usr/bin/time -l`   | Single run after hyperfine                     |
| Phase breakdown | seqtable `--profile` | count/prepare/output time + RSS                |

## Test Grid

**Files**: 3 sizes × 3 unique ratios × 2 seq lengths = 18 fixtures

| Size        | Reads | Use case            |
| ----------- | ----- | ------------------- |
| small (sm)  | 1M    | Quick iteration     |
| medium (md) | 20M   | Realistic miRNA-seq |
| large (lg)  | 100M  | Stress test         |

**Tools**: 4 tools × thread variants = 11 configurations per file

| Tool         | 1t  | 4t  | auto | Notes                                   |
| ------------ | --- | --- | ---- | --------------------------------------- |
| seqtable     | ✅  | ✅  | ✅   | Native HashMap counting                 |
| seqkit       | ✅  | ✅  | ✅   | fx2tab → sort → uniq -c pipeline        |
| awk          | ✅  | —   | —    | Single-process associative array        |
| awk+parallel | —   | ✅  | ✅   | GNU parallel with FASTQ-aware splitting |

## Known Limitations

- **seqkit comparison**: seqkit has no built-in count command, so the pipeline (`fx2tab | sort | uniq -c`) includes O(n log n) sort cost that seqtable avoids with O(n) HashMap. This reflects realistic usage, not algorithmic parity.
- **awk+parallel merge**: the two-phase merge (`parallel ... | awk merge`) adds overhead not present in seqtable's single-process merge.
- **gzip decoding**: seqtable uses built-in flate2, others use system `gzip -dc` pipe. Both are realistic but not identical implementations.
- **Peak RSS measurement**: `/usr/bin/time -l` measures the entire process tree. For piped commands (seqkit, awk), this may undercount total memory across all pipe stages.
