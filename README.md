# seqtable

🧬 High-performance parallel FASTA/FASTQ sequence counter with multiple output formats

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Features

- ✨ **Fast**: Parallel processing with Rayon (5-10x speedup on multi-core systems)
- 💾 **Memory Efficient**: Streaming I/O with constant memory usage
- 📊 **Multiple Formats**: Parquet, CSV, TSV output
- 📈 **RPM Calculation**: Optional Reads Per Million normalization
- 🗜️ **Compression**: Native support for `.gz` files
- 🎯 **Simple**: Single binary with no dependencies

## Installation

### Using Nix (Recommended)

```bash
# Install from this repository
nix profile install github:mulatta/seqtable

# Or run directly
nix run github:mulatta/seqtable -- --help
```

### From Source

```bash
git clone https://github.com/mulatta/seqtable
-cd seqtable
cd seqtable
cargo build --release
./target/release/seqtable --help
```

## Quick Start

### Basic Usage

```bash
# Count sequences in a FASTQ file
seqtable input.fastq.gz

# Specify output directory
seqtable input.fastq.gz -o results/

# Use CSV format with RPM
seqtable input.fastq.gz -f csv --rpm
```

### Multiple Files

Use GNU parallel for processing multiple files:

```bash
# Process all FASTQ files in parallel (4 jobs)
parallel -j 4 seqtable {} -o results/ ::: *.fastq.gz

# Memory-aware processing
parallel --memfree 4G seqtable {} -o results/ ::: *.fq.gz
```

## Usage

```
seqtable [OPTIONS] <INPUT>...

Arguments:
  <INPUT>...  Input file path(s) - FASTA/FASTQ/FASTQ.gz

Options:
  -o, --output-dir <DIR>        Output directory [default: .]
  -s, --suffix <SUFFIX>         Output filename suffix [default: _counts]
  -f, --format <FORMAT>         Output format [default: parquet]
                                [possible values: parquet, csv, tsv]
  -c, --chunk-size <SIZE>       Chunk size for parallel processing [default: 50000]
  -t, --threads <N>             Number of threads (0 = auto) [default: 0]
  -q, --quiet                   Disable progress bar
  --compression <TYPE>          Parquet compression [default: snappy]
                                [possible values: none, snappy, gzip, brotli, zstd]
  --rpm                         Calculate RPM (Reads Per Million)
  -h, --help                    Print help
  -V, --version                 Print version
```

## Examples

### Output Formats

```bash
# Parquet (default, best for data analysis)
seqtable input.fq.gz

# CSV (spreadsheet-friendly)
seqtable input.fq.gz -f csv

# TSV (tab-separated)
seqtable input.fq.gz -f tsv
```

### With RPM Calculation

```bash
# Add RPM column for normalization
seqtable input.fq.gz --rpm -f csv

# Output includes:
# sequence,count,rpm
# ATCGATCG,1000000,50000.00
# GCTAGCTA,500000,25000.00
```

### Custom Output

```bash
# Custom output name and location
seqtable sample.fq.gz -o results/ -s .counts -f parquet

# Output: results/sample.counts.parquet
```

### Performance Tuning

```bash
# Use 8 threads
seqtable input.fq.gz -t 8

# Larger chunks for big files (reduces overhead)
seqtable huge_file.fq.gz -c 100000

# Smaller chunks for memory-constrained systems
seqtable input.fq.gz -c 10000
```

## Output Format

### Parquet (default)

Columnar format optimized for analytics:

- Efficient compression
- Fast queries with tools like DuckDB, Polars
- Schema preservation

```python
# Read in Python
import polars as pl
df = pl.read_parquet("output_counts.parquet")
print(df.head())
```

### CSV/TSV

Human-readable text formats:

```csv
sequence,count,rpm
ATCGATCGATCG,1500000,75000.00
GCTAGCTAGCTA,1000000,50000.00
TTAATTAATTAA,500000,25000.00
```

## Performance

Typical performance on a 16-core system:

| File Size | Reads | Time  | Memory |
| --------- | ----- | ----- | ------ |
| 1 GB      | 10M   | ~15s  | ~500MB |
| 10 GB     | 100M  | ~60s  | ~2GB   |
| 100 GB    | 1B    | ~600s | ~2GB   |

**Key Features:**

- Linear scaling with CPU cores
- Constant memory usage regardless of file size
- Efficient handling of gzip-compressed files

## File Format Support

| Format   | Extension       | Compression | Streaming |
| -------- | --------------- | ----------- | --------- |
| FASTA    | `.fa`, `.fasta` | ❌          | ✅        |
| FASTQ    | `.fq`, `.fastq` | ❌          | ✅        |
| FASTA.gz | `.fa.gz`        | ✅          | ✅        |
| FASTQ.gz | `.fq.gz`        | ✅          | ✅        |

## Architecture

### Processing Pipeline

```
Input File(s)
    ↓
Streaming Reader (needletail)
    ↓
Chunking (50K sequences)
    ↓
Parallel Counting (Rayon + AHashMap)
    ↓
Parallel Merge
    ↓
Optional RPM Calculation
    ↓
Output (Parquet/CSV/TSV)
```

### Memory Usage

- **Base**: ~100MB (program overhead)
- **Chunks**: `chunk_size × threads × ~80 bytes`
- **HashMap**: `unique_sequences × ~100 bytes`
- **Total**: Typically 1-3GB for large files

### Key Optimizations

1.  **Streaming I/O**: Files processed incrementally
2.  **Parallel Hashing**: Multi-threaded counting with AHash
3.  **Zero-Copy**: Minimal data duplication
4.  **Adaptive Chunking**: Optimal chunk size selection

## Development

### Building

```bash
# Debug build
nix develop
cargo build

# Release build with optimizations
cargo build --release

# With mold linker (faster)
mold -run cargo build --release
```

### Testing

```bash
# Run tests
cargo test

# Generate test data
head -n 4000 input.fastq > test_small.fastq
seqtable test_small.fastq --rpm -f csv
```

### Benchmarking

```bash
# Time comparison
time seqtable large.fq.gz -t 1    # Single thread
time seqtable large.fq.gz -t 16   # 16 threads

# Memory profiling
/usr/bin/time -v seqtable input.fq.gz
```

## Troubleshooting

### Out of Memory

```bash
# Reduce chunk size
seqtable input.fq.gz -c 10000

# Use fewer threads
seqtable input.fq.gz -t 4
```

### Slow Performance

```bash
# Increase threads
seqtable input.fq.gz -t $(nproc)

# Larger chunks (for large files)
seqtable input.fq.gz -c 100000

# Check I/O bottleneck
iostat -x 1
```

### File Format Issues

```bash
# Verify file format
head -n 4 input.fq.gz | gunzip

# Test with small sample
head -n 40000 input.fq.gz | gunzip > test.fq
seqtable test.fq
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Citation

If you use this tool in your research, please cite:

## Acknowledgments

- [needletail](https://github.com/onecodex/needletail) - Fast FASTA/FASTQ parsing
- [rayon](https://github.com/rayon-rs/rayon) - Data parallelism
- [arrow-rs](https://github.com/apache/arrow-rs) - Parquet support

## See Also

- [seqkit](https://github.com/shenwei356/seqkit) - FASTA/FASTQ toolkit
- [fastp](https://github.com/OpenGene/fastp) - Fast preprocessing
- [bbmap](https://jgi.doe.gov/data-and-tools/software-tools/bbtools/) - Comprehensive toolkit
