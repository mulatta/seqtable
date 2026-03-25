#!/usr/bin/env python3
"""Generate FASTQ test fixtures for seqtable benchmarks.

Small fixtures (git-tracked) are deterministic for correctness tests.
Medium/large fixtures (gitignored) are for benchmarks only.

Sequence length profiles:
  short  = 22bp fixed    (siRNA/sgRNA-like)
  amplicon = 50-250bp      (amplicon/RNA-seq-like)
"""

import argparse
import gzip
import random
from pathlib import Path

BASES = "ACGT"
QUAL_CHAR = "I"  # Phred 40, uniform quality


def random_seq(length: int, rng: random.Random) -> str:
    return "".join(rng.choice(BASES) for _ in range(length))


def generate_fastq(
    output: Path,
    num_reads: int,
    num_unique: int,
    seq_length: int | tuple[int, int] = 22,
    seed: int = 42,
    compress: bool = False,
):
    rng = random.Random(seed)

    # Generate unique sequences with fixed or variable length
    if isinstance(seq_length, tuple):
        lo, hi = seq_length
        unique_seqs = [random_seq(rng.randint(lo, hi), rng) for _ in range(num_unique)]
    else:
        unique_seqs = [random_seq(seq_length, rng) for _ in range(num_unique)]

    # Assign reads to sequences (weighted toward first few for low-unique scenarios)
    weights = [1.0 / (i + 1) for i in range(num_unique)]
    total_w = sum(weights)
    weights = [w / total_w for w in weights]

    opener = gzip.open if compress else open
    mode = "wt" if compress else "w"

    with opener(output, mode) as f:
        for i in range(num_reads):
            seq = rng.choices(unique_seqs, weights=weights, k=1)[0]
            qual = QUAL_CHAR * len(seq)
            f.write(f"@read_{i}\n{seq}\n+\n{qual}\n")

    # Print summary
    size = output.stat().st_size
    unit = "bytes"
    if size > 1024 * 1024:
        size = size / (1024 * 1024)
        unit = "MB"
    elif size > 1024:
        size = size / 1024
        unit = "KB"
    len_desc = (
        f"{seq_length}bp"
        if isinstance(seq_length, int)
        else f"{seq_length[0]}-{seq_length[1]}bp"
    )
    print(
        f"  {output.name}: {num_reads} reads, {num_unique} unique, {len_desc}, {size:.1f} {unit}"
    )


# Fixture definitions: (filename, reads, unique, seq_length, compress)
# seq_length: int for fixed, (min, max) for variable
FIXTURES = {
    "small": [
        # Short fixed-length (siRNA/sgRNA)
        ("small_low_uniq.fastq", 100, 5, 22, False),
        ("small_high_uniq.fastq", 100, 90, 22, False),
        # Variable length (amplicon-like)
        ("small_amplicon.fastq", 100, 50, (50, 250), False),
    ],
    "medium": [
        # Short, low unique (siRNA-like)
        ("med_short_low.fq.gz", 1_000_000, 500, 22, True),
        # Short, high unique
        ("med_short_high.fq.gz", 1_000_000, 900_000, 22, True),
        # Variable length, low unique (amplicon)
        ("med_amplicon_low.fq.gz", 1_000_000, 500, (100, 300), True),
        # Variable length, high unique (RNA-seq-like)
        ("med_amplicon_high.fq.gz", 1_000_000, 900_000, (50, 150), True),
    ],
    "large": [
        # Short, low unique
        ("large_short_low.fq.gz", 10_000_000, 5_000, 22, True),
        # Short, high unique
        ("large_short_high.fq.gz", 10_000_000, 9_000_000, 22, True),
        # Variable length, low unique
        ("large_amplicon_low.fq.gz", 10_000_000, 5_000, (100, 300), True),
        # Variable length, high unique
        ("large_amplicon_high.fq.gz", 10_000_000, 9_000_000, (50, 150), True),
    ],
}


def main():
    parser = argparse.ArgumentParser(description="Generate FASTQ test fixtures")
    parser.add_argument(
        "--size",
        choices=["small", "medium", "large", "all"],
        default="small",
        help="Fixture size to generate",
    )
    parser.add_argument(
        "--outdir",
        type=Path,
        default=Path(__file__).parent,
        help="Output directory",
    )
    args = parser.parse_args()

    outdir = args.outdir
    outdir.mkdir(parents=True, exist_ok=True)

    targets = []
    if args.size == "all":
        for v in FIXTURES.values():
            targets.extend(v)
    else:
        targets = FIXTURES[args.size]

    for name, reads, unique, seqlen, compress in targets:
        path = outdir / name
        if path.exists():
            print(f"  {name}: already exists, skipping")
            continue
        print(f"Generating {name}...")
        generate_fastq(path, reads, unique, seqlen, seed=42, compress=compress)


if __name__ == "__main__":
    main()
