#![allow(clippy::collapsible_if)]

use ahash::AHashMap;
use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use needletail::parse_fastx_file;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

mod output;
use output::{OutputFormat, SequenceRecord};

/// High-performance FASTA/FASTQ sequence counter with parallel processing
#[derive(Parser, Debug)]
#[command(name = "seqtable")]
#[command(author = "Seungwon Lee")]
#[command(version = "0.1.1")]
#[command(about = "High performance FASTA/FASTQ sequence count table generator", long_about = None)]
struct Args {
    /// Input file path(s) - FASTA/FASTQ/FASTQ.gz formats supported
    #[arg(required = true)]
    input: Vec<PathBuf>,

    /// Output directory (default: current directory)
    #[arg(short, long, default_value = ".")]
    output_dir: PathBuf,

    /// Output filename suffix
    #[arg(short = 's', long, default_value = "_counts")]
    suffix: String,

    /// Output format
    #[arg(short = 'f', long, default_value = "parquet")]
    format: OutputFormat,

    /// Chunk size for memory/speed tradeoff (0 = auto)
    #[arg(short, long, default_value = "0")]
    chunk_size: usize,

    /// Number of threads to use (0 = auto-detect, considering parallel jobs)
    #[arg(short, long, default_value = "0")]
    threads: usize,

    /// Disable progress bar
    #[arg(short, long)]
    quiet: bool,

    /// Compression type for Parquet (none, snappy, gzip, brotli, zstd)
    #[arg(long, default_value = "snappy")]
    compression: String,

    /// Calculate and include RPM (Reads Per Million) column
    #[arg(long)]
    rpm: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Configure thread pool with intelligent defaults
    let num_threads = calculate_optimal_threads(args.threads);

    if num_threads > 0 {
        rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build_global()
            .context("Failed to initialize thread pool")?;
    }

    // Create output directory
    std::fs::create_dir_all(&args.output_dir).context("Failed to create output directory")?;

    // Print header (respect quiet flag)
    if !args.quiet {
        println!("🧬 seqtable v0.1.1");
        println!("📁 Input files: {}", args.input.len());
        println!("🧵 Threads per file: {}", rayon::current_num_threads());
        println!("📊 Output format: {:?}", args.format);
        if args.rpm {
            println!("📈 RPM calculation: enabled");
        }
        if args.chunk_size == 0 {
            println!("🎯 Adaptive chunking: enabled");
        }
        println!();
    }

    // Process each file
    for input_file in &args.input {
        process_file(input_file, &args)?;
    }

    if !args.quiet {
        println!("\n✅ All files processed successfully!");
    }
    Ok(())
}

/// Calculate optimal thread count based on system resources and parallel jobs
fn calculate_optimal_threads(requested: usize) -> usize {
    if requested > 0 {
        return requested;
    }

    // Check for RAYON_NUM_THREADS environment variable (set by user or parallel)
    if let Ok(env_threads) = std::env::var("RAYON_NUM_THREADS") {
        if let Ok(n) = env_threads.parse::<usize>() {
            return n;
        }
    }

    // Detect if running under GNU parallel or similar
    let total_cores = num_cpus::get();

    // Check for common parallel execution indicators
    let parallel_jobs = std::env::var("PARALLEL_SEQ")
        .ok()
        .and_then(|_| {
            // If PARALLEL_SEQ exists, we're in GNU parallel
            // Try to get total jobs from PARALLEL environment
            std::env::var("PARALLEL")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
        })
        .unwrap_or(1);

    if parallel_jobs > 1 {
        // Running under parallel, divide threads
        let threads_per_job = (total_cores / parallel_jobs).max(1);
        return threads_per_job;
    }

    // Default: use all cores
    total_cores
}

/// Calculate adaptive chunk size based on estimated file size
fn calculate_chunk_size(file_size: u64, requested: usize) -> usize {
    if requested > 0 {
        return requested;
    }

    // Estimate number of records (assuming ~100 bytes per record)
    let estimated_records = (file_size / 100).max(100);

    match estimated_records {
        0..=10_000 => 0,                  // No chunking for tiny files
        10_001..=100_000 => 10_000,       // Small files
        100_001..=1_000_000 => 25_000,    // Medium files
        1_000_001..=10_000_000 => 50_000, // Large files
        _ => 100_000,                     // Very large files
    }
}

fn process_file(input_path: &Path, args: &Args) -> Result<()> {
    let start_time = Instant::now();

    if !args.quiet {
        println!("📄 Processing: {}", input_path.display());
    }

    // Generate output filename
    let base_name = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output")
        .replace(".fastq", "")
        .replace(".fq", "")
        .replace(".fa", "");

    let extension = args.format.extension();
    let output_filename = format!("{}{}.{}", base_name, args.suffix, extension);
    let output_path = args.output_dir.join(output_filename);

    // Get file size for adaptive chunk size calculation
    let file_size = std::fs::metadata(input_path)?.len();
    let chunk_size = calculate_chunk_size(file_size, args.chunk_size);

    if !args.quiet && args.chunk_size == 0 {
        println!(
            "   🎯 Adaptive chunk size: {}",
            if chunk_size == 0 {
                "disabled (small file)".to_string()
            } else {
                format!("{} sequences", chunk_size)
            }
        );
    }

    // Count sequences
    let (counts, total_reads) = count_sequences(input_path, chunk_size, !args.quiet)?;

    // Convert to records with optional RPM
    let records = prepare_records(&counts, total_reads, args.rpm);

    // Save in specified format
    output::save_output(&records, &output_path, args)?;

    if !args.quiet {
        let duration = start_time.elapsed();
        println!(
            "   ✓ {} unique sequences, {} total reads → {}",
            counts.len(),
            total_reads,
            output_path.display()
        );
        println!("   ⏱️  Processing time: {:.2}s\n", duration.as_secs_f64());
    }

    Ok(())
}

#[allow(clippy::collapsible_if)]
fn count_sequences(
    file_path: &Path,
    chunk_size: usize,
    show_progress: bool,
) -> Result<(AHashMap<String, u64>, u64)> {
    let mut reader = parse_fastx_file(file_path)
        .context(format!("Failed to open file: {}", file_path.display()))?;

    // Small file optimization: no chunking
    if chunk_size == 0 {
        return count_sequences_sequential(file_path, show_progress);
    }

    // Estimate total records for progress bar
    let file_size = std::fs::metadata(file_path)?.len();
    let estimated_records = (file_size / 100).max(1000);

    let progress = if show_progress {
        let pb = ProgressBar::new(estimated_records);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("   {spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} ({percent}%)")
                .unwrap()
                .progress_chars("#>-"),
        );
        Some(pb)
    } else {
        None
    };

    // Read records in chunks
    let mut chunks = Vec::new();
    let mut current_chunk = Vec::with_capacity(chunk_size);
    let mut total_records = 0u64;

    while let Some(record) = reader.next() {
        let record = record.context("Failed to read record")?;
        let seq = String::from_utf8_lossy(&record.seq()).to_string();
        current_chunk.push(seq);
        total_records += 1;

        // Update progress bar
        if let Some(ref pb) = progress {
            if total_records % 10000 == 0 {
                pb.set_position(total_records);
            }
        }

        if current_chunk.len() >= chunk_size {
            chunks.push(std::mem::take(&mut current_chunk));
            current_chunk = Vec::with_capacity(chunk_size);
        }
    }

    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    if let Some(pb) = progress {
        pb.finish_and_clear();
    }

    if show_progress {
        println!("   📊 Total records: {}", total_records);
        print!("   🔄 Parallel processing ({} chunks)...", chunks.len());
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }

    // Parallel counting
    let results: Vec<AHashMap<String, u64>> = chunks
        .par_iter()
        .map(|chunk| {
            let mut local_counts = AHashMap::with_capacity(chunk.len() / 2);
            for seq in chunk {
                *local_counts.entry(seq.clone()).or_insert(0) += 1;
            }
            local_counts
        })
        .collect();

    // Parallel merge
    let final_counts = results
        .into_par_iter()
        .reduce(AHashMap::new, |mut acc, map| {
            for (seq, count) in map {
                *acc.entry(seq).or_insert(0) += count;
            }
            acc
        });

    if show_progress {
        println!(" Done!");
    }

    Ok((final_counts, total_records))
}

/// Fast path for small files - no chunking, single-threaded
fn count_sequences_sequential(
    file_path: &Path,
    show_progress: bool,
) -> Result<(AHashMap<String, u64>, u64)> {
    let mut reader = parse_fastx_file(file_path)
        .context(format!("Failed to open file: {}", file_path.display()))?;

    if show_progress {
        println!("   📊 Processing (sequential mode for small file)...");
    }

    let mut counts = AHashMap::new();
    let mut total_records = 0u64;

    while let Some(record) = reader.next() {
        let record = record.context("Failed to read record")?;
        let seq = String::from_utf8_lossy(&record.seq()).to_string();
        *counts.entry(seq).or_insert(0) += 1;
        total_records += 1;
    }

    if show_progress {
        println!("   📊 Total records: {}", total_records);
    }

    Ok((counts, total_records))
}

fn prepare_records(
    counts: &AHashMap<String, u64>,
    total_reads: u64,
    include_rpm: bool,
) -> Vec<SequenceRecord> {
    let mut records: Vec<_> = counts
        .iter()
        .map(|(seq, count)| {
            let rpm = if include_rpm {
                Some((*count as f64 / total_reads as f64) * 1_000_000.0)
            } else {
                None
            };
            SequenceRecord {
                sequence: seq.clone(),
                count: *count,
                rpm,
            }
        })
        .collect();

    // Sort by count (descending)
    records.sort_unstable_by(|a, b| b.count.cmp(&a.count));
    records
}
