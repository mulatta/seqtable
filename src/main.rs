use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use console::Term;
use rayon::prelude::*;
use seqtable::output::OutputFormat;
use seqtable::{
    FASTQ_EXTENSIONS, calculate_chunk_size, count_sequences, format_count, prepare_records,
    validate_fastq,
};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// High-performance FASTQ sequence counter with parallel processing
#[derive(Parser, Debug)]
#[command(name = "seqtable")]
#[command(version)]
#[command(about = "Count sequences in FASTQ files")]
struct Args {
    /// Input FASTQ file path(s) (.fastq, .fq, .fastq.gz, .fq.gz)
    #[arg(required = true)]
    input: Vec<PathBuf>,

    /// Output directory
    #[arg(short, long, default_value = ".")]
    output_dir: PathBuf,

    /// Output format
    #[arg(short = 'f', long, default_value = "parquet")]
    format: OutputFormat,

    /// Number of threads (0 = auto)
    #[arg(short, long, default_value = "0")]
    threads: usize,

    /// Suppress all status output
    #[arg(short, long)]
    quiet: bool,

    /// Parquet compression
    #[arg(long, default_value = "zstd")]
    compression: ParquetCompression,

    /// Include RPM (Reads Per Million) column
    #[arg(long)]
    rpm: bool,
}

#[derive(Debug, Clone, ValueEnum)]
enum ParquetCompression {
    None,
    Snappy,
    Gzip,
    Brotli,
    Zstd,
}

impl ParquetCompression {
    fn to_parquet(&self) -> parquet::basic::Compression {
        match self {
            ParquetCompression::None => parquet::basic::Compression::UNCOMPRESSED,
            ParquetCompression::Snappy => parquet::basic::Compression::SNAPPY,
            ParquetCompression::Gzip => {
                parquet::basic::Compression::GZIP(parquet::basic::GzipLevel::default())
            }
            ParquetCompression::Brotli => {
                parquet::basic::Compression::BROTLI(parquet::basic::BrotliLevel::default())
            }
            ParquetCompression::Zstd => {
                parquet::basic::Compression::ZSTD(parquet::basic::ZstdLevel::default())
            }
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let quiet = args.quiet;
    let is_tty = Term::stderr().is_term();

    if args.threads > 0 {
        rayon::ThreadPoolBuilder::new()
            .num_threads(args.threads)
            .build_global()
            .context("Failed to initialize thread pool")?;
    }

    std::fs::create_dir_all(&args.output_dir).context("Failed to create output directory")?;

    if !quiet {
        eprintln!(
            "seqtable {} | {} file{} | {} threads | {:?}",
            env!("CARGO_PKG_VERSION"),
            args.input.len(),
            if args.input.len() > 1 { "s" } else { "" },
            rayon::current_num_threads(),
            args.format
        );
        eprintln!();
    }

    let total_start = Instant::now();

    let n_files = args.input.len();
    if n_files == 1 {
        process_file(&args.input[0], &args, 1, n_files, quiet, is_tty)?;
    } else {
        args.input
            .par_iter()
            .enumerate()
            .try_for_each(|(idx, input_file)| {
                // Disable progress bars in parallel mode to avoid interleaving
                process_file(input_file, &args, idx + 1, n_files, quiet, false)
            })?;
    }

    if !quiet {
        eprintln!(
            "completed {} file{} in {:.2}s",
            args.input.len(),
            if args.input.len() > 1 { "s" } else { "" },
            total_start.elapsed().as_secs_f64()
        );
    }

    Ok(())
}

fn process_file(
    input_path: &Path,
    args: &Args,
    file_num: usize,
    total_files: usize,
    quiet: bool,
    is_tty: bool,
) -> Result<()> {
    validate_fastq(input_path)?;
    let start_time = Instant::now();

    let base_name = {
        let name = input_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        let mut base = name;
        for suffix in FASTQ_EXTENSIONS {
            if let Some(stripped) = base.strip_suffix(suffix) {
                base = stripped;
                break;
            }
        }
        base.to_string()
    };

    let extension = args.format.extension();
    let output_filename = format!("{base_name}.{extension}");
    let output_path = args.output_dir.join(&output_filename);
    let input_display = input_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("?");

    if !quiet {
        eprintln!("[{}/{}] {}", file_num, total_files, input_display);
    }

    let file_size = std::fs::metadata(input_path)?.len();
    let chunk_size = calculate_chunk_size(file_size);

    // Count
    let show_progress = !quiet && is_tty;
    let (counts, total_reads) = count_sequences(input_path, chunk_size, show_progress)?;

    let unique_count = counts.len() as u64;

    // Prepare
    let records = prepare_records(counts);

    // Output
    if !quiet {
        eprint!("      {:<10} {} ... ", "writing", output_filename);
        std::io::Write::flush(&mut std::io::stderr()).ok();
    }
    seqtable::output::save_output(
        &records,
        &output_path,
        &args.format,
        args.compression.to_parquet(),
        total_reads,
        args.rpm,
    )?;
    if !quiet {
        eprintln!("done");
    }

    if !quiet {
        let duration = start_time.elapsed();
        eprintln!(
            "      {:<10} {} unique | {} total -> {} [{:.2}s]",
            "result",
            format_count(unique_count),
            format_count(total_reads),
            output_filename,
            duration.as_secs_f64()
        );
        eprintln!();
    }

    Ok(())
}
