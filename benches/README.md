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
2. **Correctness verified** — every tool's output is compared against awk ground truth. Mismatches are flagged.
3. **Fixed thread counts** — 1t/4t/auto for reproducible scaling comparison.
4. **Realistic usage** — each tool uses its natural invocation pattern.
5. **Statistical rigor** — warmup=3, runs=5, `sync` between runs. Mean + stddev reported.

## What We Measure

| Metric          | Tool                 | Notes                                          |
| --------------- | -------------------- | ---------------------------------------------- |
| Wall time       | hyperfine            | Includes warmup, statistical outlier detection  |
| Peak RSS        | `/usr/bin/time -l`   | Single run after hyperfine                      |
| Phase breakdown | seqtable `--profile` | count/prepare/output time + RSS                 |

## Test Grid

**Files**: 3 sizes x 3 unique ratios x 2 seq lengths = 18 fixtures

| Size        | Reads | Use case            |
| ----------- | ----- | ------------------- |
| small (sm)  | 1M    | Quick iteration     |
| medium (md) | 20M   | Realistic miRNA-seq |
| large (lg)  | 100M  | Stress test         |

**Tools**: 4 tools x thread variants = 9 configurations per file

| Tool      | 1t  | 4t  | auto | Notes                                      |
| --------- | --- | --- | ---- | ------------------------------------------ |
| seqtable  | Y   | Y   | Y    | Native HashMap counting                    |
| seqkit    | Y   | Y   | Y    | fx2tab pipe to sort/uniq -c pipeline       |
| awk       | Y   | -   | -    | HashMap counting (associative array)       |
| coreutils | Y   | Y   | -    | sort/uniq -c baseline (sort --parallel=4)  |

## Why These Tools?

- **coreutils (sort|uniq -c)**: The true baseline. POSIX standard, universally available, correct by construction. Single-threaded and parallel sort variants.
- **awk**: HashMap-based counting in a single process. Shows the O(n) algorithm advantage over O(n log n) sort, but limited to single core.
- **seqkit**: The most widely used bioinformatics FASTQ toolkit. Realistic comparison for users choosing between tools.
- **seqtable**: Our tool. Should beat all of the above on both speed and correctness.

## Why Not awk+parallel?

GNU parallel's `--pipe` splits input on line boundaries, but FASTQ records are 4 lines.
A block split mid-record corrupts `NR%4==2` counting in downstream awk processes,
producing incorrect results. We verified this: line counts consistently mismatch ground truth.
Since correctness is non-negotiable for a benchmark comparison, awk+parallel is excluded.

## Known Limitations

- **seqkit comparison**: seqkit has no built-in count command, so the pipeline (`fx2tab | sort | uniq -c`) includes O(n log n) sort cost that seqtable avoids with O(n) HashMap. This reflects realistic usage, not algorithmic parity.
- **gzip decoding**: seqtable uses built-in flate2, others use system `gzip -dc` pipe. Both are realistic but not identical implementations.
- **Peak RSS measurement**: `/usr/bin/time -l` measures the entire process tree. For piped commands (seqkit, awk), this may undercount total memory across all pipe stages.
- **Warmup warnings**: On macOS, there is no way to drop page cache without root. `sync` only flushes write buffers. First-run slowness after warmup is expected; stddev indicates measurement reliability.
