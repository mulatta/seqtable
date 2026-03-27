pub mod output;

pub use output::{OutputFormat, SequenceRecord};

use ahash::AHashMap;
use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use needletail::parse_fastx_file;
use rayon::prelude::*;
use std::path::Path;

pub type SeqCounts = AHashMap<Vec<u8>, u64>;

pub const FASTQ_EXTENSIONS: &[&str] = &[".fastq.gz", ".fq.gz", ".fastq", ".fq"];

pub fn validate_fastq(path: &Path) -> Result<()> {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let is_fastq = FASTQ_EXTENSIONS.iter().any(|ext| name.ends_with(ext));
    anyhow::ensure!(
        is_fastq,
        "unsupported file format: {}\n  expected: .fastq, .fq, .fastq.gz, .fq.gz",
        path.display()
    );
    Ok(())
}

pub fn calculate_chunk_size(file_size: u64) -> usize {
    let estimated_records = (file_size / 100).max(100);
    match estimated_records {
        0..=10_000 => 0,
        10_001..=100_000 => 10_000,
        100_001..=1_000_000 => 25_000,
        1_000_001..=10_000_000 => 50_000,
        _ => 100_000,
    }
}

pub fn format_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

#[allow(clippy::collapsible_if)]
pub fn count_sequences(
    file_path: &Path,
    chunk_size: usize,
    show_progress: bool,
) -> Result<(SeqCounts, u64)> {
    let mut reader = parse_fastx_file(file_path)
        .with_context(|| format!("Failed to open file: {}", file_path.display()))?;

    if chunk_size == 0 {
        return count_sequences_sequential(file_path, show_progress);
    }

    let file_size = std::fs::metadata(file_path)?.len();
    let estimated_records = (file_size / 100).max(1000);

    let progress = if show_progress {
        let pb = ProgressBar::new(estimated_records);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("      {msg:<10} [{bar:30}] {pos}/{len}")
                .unwrap()
                .progress_chars("=> "),
        );
        pb.set_message("reading");
        Some(pb)
    } else {
        None
    };

    let mut partial_counts: Vec<SeqCounts> = Vec::new();
    let mut local = AHashMap::with_capacity(chunk_size / 2);
    let mut chunk_count = 0usize;
    let mut total_records = 0u64;

    while let Some(record) = reader.next() {
        let record = record.context("Failed to read record")?;
        let raw = record.seq();
        total_records += 1;

        // Probe existing key first — duplicates skip allocation entirely
        if let Some(count) = local.get_mut(raw.as_ref()) {
            *count += 1;
        } else {
            local.insert(raw.into_owned(), 1);
        }
        chunk_count += 1;

        if let Some(ref pb) = progress {
            if total_records.is_multiple_of(10000) {
                pb.set_position(total_records);
            }
        }

        if chunk_count >= chunk_size {
            partial_counts.push(std::mem::take(&mut local));
            local = AHashMap::with_capacity(chunk_size / 2);
            chunk_count = 0;
        }
    }

    // Count remaining
    if !local.is_empty() {
        partial_counts.push(local);
    }

    let num_chunks = partial_counts.len();

    if let Some(ref pb) = progress {
        pb.set_position(total_records);
        pb.set_message("merging");
    }

    let final_counts = partial_counts
        .into_par_iter()
        .reduce(AHashMap::new, |mut acc, map| {
            for (seq, count) in map {
                *acc.entry(seq).or_insert(0) += count;
            }
            acc
        });

    if let Some(pb) = progress {
        pb.finish_and_clear();
        eprintln!(
            "      {:<10} {} records | {} chunks",
            "read",
            format_count(total_records),
            num_chunks
        );
    }

    Ok((final_counts, total_records))
}

pub fn count_sequences_sequential(
    file_path: &Path,
    show_progress: bool,
) -> Result<(SeqCounts, u64)> {
    let mut reader = parse_fastx_file(file_path)
        .with_context(|| format!("Failed to open file: {}", file_path.display()))?;

    if show_progress {
        eprint!("      {:<10} ... ", "counting");
        std::io::Write::flush(&mut std::io::stderr()).ok();
    }

    let mut counts: SeqCounts = AHashMap::new();
    let mut total_records = 0u64;

    while let Some(record) = reader.next() {
        let record = record.context("Failed to read record")?;
        let raw = record.seq();
        if let Some(count) = counts.get_mut(raw.as_ref()) {
            *count += 1;
        } else {
            counts.insert(raw.into_owned(), 1);
        }
        total_records += 1;
    }

    if show_progress {
        eprintln!("{} records", format_count(total_records));
    }

    Ok((counts, total_records))
}

pub fn prepare_records(
    counts: SeqCounts,
    total_reads: u64,
    include_rpm: bool,
) -> Vec<SequenceRecord> {
    let mut records: Vec<_> = counts
        .into_iter()
        .map(|(seq, count)| {
            let rpm = if include_rpm {
                Some((count as f64 / total_reads as f64) * 1_000_000.0)
            } else {
                None
            };
            SequenceRecord {
                sequence: seq,
                count,
                rpm,
            }
        })
        .collect();

    records.sort_unstable_by(|a, b| b.count.cmp(&a.count));
    records
}
