pub mod output;

pub use output::{OutputFormat, SequenceRecord};

use ahash::AHashMap;
use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use needletail::parse_fastx_file;
use std::path::Path;

/// Dual HashMap: packed u128 keys for short ACGT-only sequences, Vec<u8> fallback for the rest.
#[derive(Clone, Default)]
pub struct DualSeqCounts {
    /// ≤32bp ACGT-only sequences packed as u128 (upper 64 bits = length, lower 64 = 2-bit encoded)
    pub short: AHashMap<u128, u64>,
    /// Fallback for >32bp or non-ACGT sequences
    pub long: AHashMap<Vec<u8>, u64>,
}

impl DualSeqCounts {
    pub fn new() -> Self {
        Self {
            short: AHashMap::new(),
            long: AHashMap::new(),
        }
    }

    pub fn with_capacity(short_cap: usize, long_cap: usize) -> Self {
        Self {
            short: AHashMap::with_capacity(short_cap),
            long: AHashMap::with_capacity(long_cap),
        }
    }

    pub fn len(&self) -> usize {
        self.short.len() + self.long.len()
    }

    pub fn is_empty(&self) -> bool {
        self.short.is_empty() && self.long.is_empty()
    }
}

/// Lookup table: ASCII byte → 2-bit encoding (0xFF = invalid)
const DNA_ENCODE: [u8; 256] = {
    let mut table = [0xFFu8; 256];
    table[b'A' as usize] = 0;
    table[b'a' as usize] = 0;
    table[b'C' as usize] = 1;
    table[b'c' as usize] = 1;
    table[b'G' as usize] = 2;
    table[b'g' as usize] = 2;
    table[b'T' as usize] = 3;
    table[b't' as usize] = 3;
    table
};

/// Pack a DNA sequence (≤32bp, ACGT-only) into a u128.
/// Upper 64 bits store the length, lower 64 bits store 2-bit encoded bases.
/// Returns None for sequences >32bp or containing non-ACGT bases.
#[inline]
pub fn pack_dna(seq: &[u8]) -> Option<u128> {
    if seq.len() > 32 {
        return None;
    }
    let mut packed: u64 = 0;
    for &base in seq {
        let bits = DNA_ENCODE[base as usize];
        if bits == 0xFF {
            return None;
        }
        packed = (packed << 2) | bits as u64;
    }
    Some((seq.len() as u128) << 64 | packed as u128)
}

/// Unpack a u128 key back into a DNA sequence.
pub fn unpack_dna(key: u128) -> Vec<u8> {
    let len = (key >> 64) as usize;
    let packed = key as u64;
    let mut seq = Vec::with_capacity(len);
    for i in (0..len).rev() {
        seq.push(match (packed >> (i * 2)) & 3 {
            0 => b'A',
            1 => b'C',
            2 => b'G',
            3 => b'T',
            _ => unreachable!(),
        });
    }
    seq
}

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

pub fn count_sequences(
    file_path: &Path,
    chunk_size: usize,
    show_progress: bool,
) -> Result<(DualSeqCounts, u64)> {
    if chunk_size == 0 {
        return count_sequences_sequential(file_path, show_progress);
    }

    let mut reader = parse_fastx_file(file_path)
        .with_context(|| format!("Failed to open file: {}", file_path.display()))?;

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

    let mut partial_short: Vec<AHashMap<u128, u64>> = Vec::new();
    let mut partial_long: Vec<AHashMap<Vec<u8>, u64>> = Vec::new();
    let mut local_short: AHashMap<u128, u64> = AHashMap::with_capacity(chunk_size / 2);
    let mut local_long: AHashMap<Vec<u8>, u64> = AHashMap::new();
    let mut chunk_count = 0usize;
    let mut total_records = 0u64;

    while let Some(record) = reader.next() {
        let record = record.context("Failed to read record")?;
        let raw = record.seq();
        total_records += 1;

        if let Some(key) = pack_dna(raw.as_ref()) {
            if let Some(count) = local_short.get_mut(&key) {
                *count += 1;
            } else {
                local_short.insert(key, 1);
            }
        } else if let Some(count) = local_long.get_mut(raw.as_ref()) {
            *count += 1;
        } else {
            local_long.insert(raw.into_owned(), 1);
        }
        chunk_count += 1;

        if let Some(ref pb) = progress
            && total_records.is_multiple_of(10000)
        {
            pb.set_position(total_records);
        }

        if chunk_count >= chunk_size {
            partial_short.push(std::mem::take(&mut local_short));
            partial_long.push(std::mem::take(&mut local_long));
            local_short = AHashMap::with_capacity(chunk_size / 2);
            local_long = AHashMap::new();
            chunk_count = 0;
        }
    }

    if !local_short.is_empty() || !local_long.is_empty() {
        partial_short.push(local_short);
        partial_long.push(local_long);
    }

    let num_chunks = partial_short.len();

    if let Some(ref pb) = progress {
        pb.set_position(total_records);
        pb.set_message("merging");
    }

    // Merge short maps
    let final_short = if partial_short.is_empty() {
        AHashMap::new()
    } else {
        let max_idx = partial_short
            .iter()
            .enumerate()
            .max_by_key(|(_, m)| m.len())
            .map(|(i, _)| i)
            .unwrap();
        let mut base = partial_short.swap_remove(max_idx);
        for map in partial_short {
            for (seq, count) in map {
                *base.entry(seq).or_insert(0) += count;
            }
        }
        base
    };

    // Merge long maps
    let final_long = if partial_long.is_empty() {
        AHashMap::new()
    } else {
        let max_idx = partial_long
            .iter()
            .enumerate()
            .max_by_key(|(_, m)| m.len())
            .map(|(i, _)| i)
            .unwrap();
        let mut base = partial_long.swap_remove(max_idx);
        for map in partial_long {
            for (seq, count) in map {
                *base.entry(seq).or_insert(0) += count;
            }
        }
        base
    };

    if let Some(pb) = progress {
        pb.finish_and_clear();
        eprintln!(
            "      {:<10} {} records | {} chunks",
            "read",
            format_count(total_records),
            num_chunks
        );
    }

    Ok((
        DualSeqCounts {
            short: final_short,
            long: final_long,
        },
        total_records,
    ))
}

pub fn count_sequences_sequential(
    file_path: &Path,
    show_progress: bool,
) -> Result<(DualSeqCounts, u64)> {
    let mut reader = parse_fastx_file(file_path)
        .with_context(|| format!("Failed to open file: {}", file_path.display()))?;

    if show_progress {
        eprint!("      {:<10} ... ", "counting");
        std::io::Write::flush(&mut std::io::stderr()).ok();
    }

    let file_size = std::fs::metadata(file_path).map(|m| m.len()).unwrap_or(0);
    let estimated_unique = (file_size / 2000).max(64) as usize;
    let mut counts = DualSeqCounts::with_capacity(estimated_unique, estimated_unique / 4);
    let mut total_records = 0u64;

    while let Some(record) = reader.next() {
        let record = record.context("Failed to read record")?;
        let raw = record.seq();
        if let Some(key) = pack_dna(raw.as_ref()) {
            if let Some(count) = counts.short.get_mut(&key) {
                *count += 1;
            } else {
                counts.short.insert(key, 1);
            }
        } else if let Some(count) = counts.long.get_mut(raw.as_ref()) {
            *count += 1;
        } else {
            counts.long.insert(raw.into_owned(), 1);
        }
        total_records += 1;
    }

    if show_progress {
        eprintln!("{} records", format_count(total_records));
    }

    Ok((counts, total_records))
}

pub fn prepare_records(
    counts: DualSeqCounts,
    total_reads: u64,
    include_rpm: bool,
) -> Vec<SequenceRecord> {
    let make_rpm = |count: u64| -> Option<f64> {
        if include_rpm {
            Some((count as f64 / total_reads as f64) * 1_000_000.0)
        } else {
            None
        }
    };

    let total_unique = counts.short.len() + counts.long.len();
    let mut records: Vec<SequenceRecord> = Vec::with_capacity(total_unique);

    // Unpack short (2-bit encoded) sequences
    for (key, count) in counts.short {
        records.push(SequenceRecord {
            sequence: unpack_dna(key),
            count,
            rpm: make_rpm(count),
        });
    }

    // Long sequences are already Vec<u8>
    for (seq, count) in counts.long {
        records.push(SequenceRecord {
            sequence: seq,
            count,
            rpm: make_rpm(count),
        });
    }

    records.sort_unstable_by(|a, b| b.count.cmp(&a.count));
    records
}
