//! Generate FASTQ test fixtures for benchmarking.
//!
//! Usage:
//!   cargo run --example generate_fixtures --release -- [--size small|medium|large|all] [--outdir DIR]
//!
//! Full grid: 3 sizes × 3 unique levels × 2 lengths = 18 files
//! Files are generated in parallel across available cores.

use flate2::Compression;
use flate2::write::GzEncoder;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

const BASES: &[u8] = b"ACGT";
const QUAL: u8 = b'I'; // Phred 40
const SEQ_CACHE_SIZE: usize = 1024;

/// Simple xorshift64 RNG (deterministic, no dependencies)
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn next_f64(&mut self) -> f64 {
        (self.next_u64() as f64) / (u64::MAX as f64)
    }
}

/// Zipf inverse transform: map uniform [0,1) → sequence index [0, n)
/// Using the harmonic number approximation: H(n) ≈ ln(n) + γ
/// For Zipf(1, n): CDF(k) = H(k) / H(n), so k = floor(exp(u * H(n) - γ))
fn zipf_sample(rng: &mut Rng, n: usize) -> usize {
    if n <= 1 {
        return 0;
    }
    let u = rng.next_f64();
    // Inverse CDF: find k such that H(k)/H(n) >= u
    // H(k) = u * H(n), k ≈ exp(H(k) - γ) but simpler: use exact formula
    // k = floor(exp(u * ln(n+1))) - 1 (approximate, good enough for fixtures)
    let k = ((n as f64 + 1.0).powf(u) - 1.0).floor() as usize;
    k.min(n - 1)
}

/// Generate a deterministic sequence for a given unique index
fn generate_seq_for_index(index: usize, seq_len: &SeqLen) -> Vec<u8> {
    let len = match seq_len {
        SeqLen::Fixed(l) => *l,
        SeqLen::Variable(lo, hi) => {
            let mut rng = Rng::new(index as u64 + 0xBEEF);
            lo + (rng.next_u64() as usize) % (hi - lo + 1)
        }
    };
    let mut rng = Rng::new(index as u64 + 1);
    (0..len)
        .map(|_| BASES[(rng.next_u64() as usize) % 4])
        .collect()
}

#[derive(Clone)]
enum SeqLen {
    Fixed(usize),
    Variable(usize, usize),
}

struct FixtureSpec {
    name: String,
    reads: usize,
    unique_count: usize,
    seq_len: SeqLen,
    size_class: &'static str,
}

fn generate_fastq(spec: &FixtureSpec, outdir: &Path) -> std::io::Result<()> {
    let path = outdir.join(&spec.name);
    if path.exists() {
        eprintln!("  {} already exists, skipping", spec.name);
        return Ok(());
    }

    let start = Instant::now();

    let is_gz = spec.name.ends_with(".gz");
    let file = BufWriter::with_capacity(1024 * 1024, File::create(&path)?);

    if is_gz {
        let mut gz = GzEncoder::new(file, Compression::fast());
        write_reads(&mut gz, spec)?;
        gz.finish()?;
    } else {
        let mut w = file;
        write_reads(&mut w, spec)?;
        w.flush()?;
    }

    let size = std::fs::metadata(&path)?.len();
    let elapsed = start.elapsed();
    eprintln!(
        "  {}: {} reads, {} unique, {}, {:.1}s",
        spec.name,
        format_num(spec.reads),
        format_num(spec.unique_count),
        format_bytes(size),
        elapsed.as_secs_f64()
    );

    Ok(())
}

fn write_reads(w: &mut impl Write, spec: &FixtureSpec) -> std::io::Result<()> {
    let mut rng = Rng::new(42);
    let mut buf = Vec::with_capacity(1024);

    // LRU-like cache for hot sequences (Zipf means top sequences are hit repeatedly)
    let mut seq_cache: HashMap<usize, Vec<u8>> = HashMap::with_capacity(SEQ_CACHE_SIZE);

    for i in 0..spec.reads {
        // O(1) Zipf sampling — no cumulative weight table needed
        let idx = zipf_sample(&mut rng, spec.unique_count);

        // Get sequence from cache or generate on-the-fly
        let seq = seq_cache
            .entry(idx)
            .or_insert_with(|| generate_seq_for_index(idx, &spec.seq_len));

        // Write FASTQ record
        buf.clear();
        buf.extend_from_slice(b"@read_");
        write!(&mut buf, "{i}")?;
        buf.push(b'\n');
        buf.extend_from_slice(seq);
        buf.push(b'\n');
        buf.extend_from_slice(b"+\n");
        buf.resize(buf.len() + seq.len(), QUAL);
        buf.push(b'\n');
        w.write_all(&buf)?;
    }
    Ok(())
}

fn format_num(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{}M", n / 1_000_000)
    } else if n >= 1_000 {
        format!("{}K", n / 1_000)
    } else {
        n.to_string()
    }
}

fn format_bytes(b: u64) -> String {
    if b >= 1024 * 1024 * 1024 {
        format!("{:.1}GB", b as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if b >= 1024 * 1024 {
        format!("{:.1}MB", b as f64 / (1024.0 * 1024.0))
    } else if b >= 1024 {
        format!("{:.1}KB", b as f64 / 1024.0)
    } else {
        format!("{}B", b)
    }
}

fn all_specs() -> Vec<FixtureSpec> {
    let sizes: &[(&str, usize, &str)] = &[
        ("bn", 10_000, "bench"),
        ("sm", 1_000_000, "small"),
        ("md", 20_000_000, "medium"),
        ("lg", 100_000_000, "large"),
    ];
    let uniques: &[(&str, f64)] = &[("low", 0.05), ("mid", 0.50), ("high", 0.90)];
    let lengths: &[(&str, SeqLen)] = &[
        ("short", SeqLen::Fixed(22)),
        ("amp", SeqLen::Variable(100, 300)),
    ];

    let mut specs = Vec::new();
    for (sz_prefix, reads, size_class) in sizes {
        for (uniq_name, uniq_frac) in uniques {
            for (len_name, seq_len) in lengths {
                let unique_count = ((*reads as f64) * uniq_frac) as usize;
                let ext = if *size_class == "small" || *size_class == "bench" {
                    "fastq"
                } else {
                    "fq.gz"
                };
                let name = format!("{sz_prefix}_{len_name}_{uniq_name}_{reads}.{ext}");
                specs.push(FixtureSpec {
                    name,
                    reads: *reads,
                    unique_count,
                    seq_len: seq_len.clone(),
                    size_class,
                });
            }
        }
    }
    specs
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut size_filter: Option<&str> = None;
    let mut outdir = PathBuf::from("tests/fixtures");

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--size" => {
                i += 1;
                size_filter = Some(match args.get(i).map(|s| s.as_str()) {
                    Some("bench") => "bench",
                    Some("small") => "small",
                    Some("medium") => "medium",
                    Some("large") => "large",
                    Some("all") | None => "all",
                    Some(other) => {
                        eprintln!("Unknown size: {other}");
                        std::process::exit(1);
                    }
                });
            }
            "--outdir" => {
                i += 1;
                if let Some(d) = args.get(i) {
                    outdir = PathBuf::from(d);
                }
            }
            _ => {}
        }
        i += 1;
    }

    std::fs::create_dir_all(&outdir).expect("create outdir");

    let specs = all_specs();
    let filter = size_filter.unwrap_or("all");

    let filtered: Vec<&FixtureSpec> = specs
        .iter()
        .filter(|s| filter == "all" || s.size_class == filter)
        .collect();

    eprintln!(
        "Generating {} fixtures (size={filter}) in {} using {} threads",
        filtered.len(),
        outdir.display(),
        rayon::current_num_threads()
    );

    // Parallel file generation (each file is independent)
    filtered.par_iter().for_each(|spec| {
        generate_fastq(spec, &outdir).expect("generate fixture");
    });

    eprintln!("Done!");
}
