#[cfg(feature = "cli")]
pub mod output;

#[cfg(feature = "cli")]
pub use output::{OutputFormat, SequenceData, SequenceRecord};

use ahash::AHashMap;
use anyhow::{Context, Result};
use needletail::parse_fastx_file;
use std::path::Path;

/// 2-bit packed DNA key: up to 160bp ACGT-only sequences in 48 bytes.
/// data[0..5] stores 2-bit encoded bases (LSB-first), len stores sequence length.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PackedDna {
    data: [u64; 5],
    len: u8,
}

/// Dual HashMap: PackedDna keys for ≤160bp ACGT-only, Vec<u8> fallback for the rest.
#[derive(Clone, Default)]
pub struct DualSeqCounts {
    pub(crate) packed: AHashMap<PackedDna, u64>,
    /// Fallback for >160bp or non-ACGT sequences
    pub(crate) long: AHashMap<Vec<u8>, u64>,
}

impl DualSeqCounts {
    pub fn new() -> Self {
        Self {
            packed: AHashMap::new(),
            long: AHashMap::new(),
        }
    }

    pub fn with_capacity(packed_cap: usize, long_cap: usize) -> Self {
        Self {
            packed: AHashMap::with_capacity(packed_cap),
            long: AHashMap::with_capacity(long_cap),
        }
    }

    pub fn len(&self) -> usize {
        self.packed.len() + self.long.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packed.is_empty() && self.long.is_empty()
    }

    /// Consume and return (sequence, count) pairs sorted by count descending.
    pub fn into_sorted_vec(self) -> Vec<(Vec<u8>, u64)> {
        let mut result = Vec::with_capacity(self.packed.len() + self.long.len());
        let mut buf = Vec::with_capacity(160);
        for (key, count) in self.packed {
            buf.clear();
            unpack_dna_into(&key, &mut buf);
            result.push((buf.clone(), count));
        }
        for (seq, count) in self.long {
            result.push((seq, count));
        }
        result.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        result
    }

    /// Insert or increment a sequence count. Packs ACGT-only ≤160bp sequences.
    #[inline]
    pub fn insert(&mut self, seq: std::borrow::Cow<'_, [u8]>) {
        if let Some(key) = pack_dna(seq.as_ref()) {
            if let Some(count) = self.packed.get_mut(&key) {
                *count += 1;
            } else {
                self.packed.insert(key, 1);
            }
        } else if let Some(count) = self.long.get_mut(seq.as_ref()) {
            *count += 1;
        } else {
            self.long.insert(seq.into_owned(), 1);
        }
    }
}

impl IntoIterator for DualSeqCounts {
    type Item = (Vec<u8>, u64);
    type IntoIter = Box<dyn Iterator<Item = (Vec<u8>, u64)>>;

    fn into_iter(self) -> Self::IntoIter {
        let mut buf = Vec::with_capacity(160);
        let packed = self.packed.into_iter().map(move |(key, count)| {
            buf.clear();
            unpack_dna_into(&key, &mut buf);
            (buf.clone(), count)
        });
        Box::new(packed.chain(self.long))
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

const MAX_PACKED_BP: usize = 160;

/// Pack a DNA sequence (≤160bp, ACGT-only) into a PackedDna.
/// Returns None for sequences >160bp or containing non-ACGT bases.
#[inline]
pub fn pack_dna(seq: &[u8]) -> Option<PackedDna> {
    if seq.len() > MAX_PACKED_BP {
        return None;
    }
    let mut data = [0u64; 5];
    for (i, &base) in seq.iter().enumerate() {
        let bits = DNA_ENCODE[base as usize];
        if bits == 0xFF {
            return None;
        }
        let word = i >> 5; // i / 32
        let shift = (i & 31) << 1; // (i % 32) * 2
        data[word] |= (bits as u64) << shift;
    }
    Some(PackedDna {
        data,
        len: seq.len() as u8,
    })
}

/// Unpack a PackedDna key into an existing buffer (avoids allocation).
pub fn unpack_dna_into(key: &PackedDna, buf: &mut Vec<u8>) {
    const BASES: [u8; 4] = [b'A', b'C', b'G', b'T'];
    let len = key.len as usize;
    buf.reserve(len);
    for i in 0..len {
        let word = i >> 5;
        let shift = (i & 31) << 1;
        let bits = (key.data[word] >> shift) & 3;
        buf.push(BASES[bits as usize]);
    }
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

fn merge_count_maps<K: Eq + std::hash::Hash>(mut parts: Vec<AHashMap<K, u64>>) -> AHashMap<K, u64> {
    if parts.is_empty() {
        return AHashMap::new();
    }
    let max_idx = parts
        .iter()
        .enumerate()
        .max_by_key(|(_, m)| m.len())
        .map(|(i, _)| i)
        .unwrap();
    let mut base = parts.swap_remove(max_idx);
    for map in parts {
        for (key, count) in map {
            *base.entry(key).or_insert(0) += count;
        }
    }
    base
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

    #[cfg(feature = "cli")]
    let estimated_records = {
        let file_size = std::fs::metadata(file_path)?.len();
        (file_size / 100).max(1000)
    };

    #[cfg(feature = "cli")]
    let progress = if show_progress {
        let pb = indicatif::ProgressBar::new(estimated_records);
        pb.set_style(
            indicatif::ProgressStyle::default_bar()
                .template("      {msg:<10} [{bar:30}] {pos}/{len}")
                .unwrap()
                .progress_chars("=> "),
        );
        pb.set_message("reading");
        Some(pb)
    } else {
        None
    };

    let mut partial_packed: Vec<AHashMap<PackedDna, u64>> = Vec::new();
    let mut partial_long: Vec<AHashMap<Vec<u8>, u64>> = Vec::new();
    let mut local = DualSeqCounts::with_capacity(chunk_size, chunk_size / 10);
    let mut chunk_count = 0usize;
    let mut total_records = 0u64;

    while let Some(record) = reader.next() {
        let record = record.context("Failed to read record")?;
        total_records += 1;
        local.insert(record.seq());
        chunk_count += 1;

        #[cfg(feature = "cli")]
        if let Some(ref pb) = progress
            && total_records & 0x3FFF == 0
        {
            pb.set_position(total_records);
        }

        if chunk_count >= chunk_size {
            partial_packed.push(std::mem::take(&mut local.packed));
            partial_long.push(std::mem::take(&mut local.long));
            local = DualSeqCounts::with_capacity(chunk_size, chunk_size / 10);
            chunk_count = 0;
        }
    }

    if !local.is_empty() {
        partial_packed.push(local.packed);
        partial_long.push(local.long);
    }

    #[cfg(feature = "cli")]
    let num_chunks = partial_packed.len();

    #[cfg(feature = "cli")]
    if let Some(ref pb) = progress {
        pb.set_position(total_records);
        pb.set_message("merging");
    }

    let final_packed = merge_count_maps(partial_packed);
    let final_long = merge_count_maps(partial_long);

    #[cfg(feature = "cli")]
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
            packed: final_packed,
            long: final_long,
        },
        total_records,
    ))
}

pub fn count_sequences_sequential(
    file_path: &Path,
    show_progress: bool,
) -> Result<(DualSeqCounts, u64)> {
    let reader = parse_fastx_file(file_path)
        .with_context(|| format!("Failed to open file: {}", file_path.display()))?;

    let file_size = std::fs::metadata(file_path).map(|m| m.len()).unwrap_or(0);
    let estimated_unique = (file_size / 2000).max(64) as usize;

    count_from_reader(reader, estimated_unique, show_progress)
}

/// Count sequences from stdin or any reader.
pub fn count_sequences_from_reader<R: std::io::Read + Send + 'static>(
    reader: R,
    show_progress: bool,
) -> Result<(DualSeqCounts, u64)> {
    let fastx =
        needletail::parse_fastx_reader(reader).context("Failed to parse FASTQ from stdin")?;

    count_from_reader(fastx, 64, show_progress)
}

fn count_from_reader(
    mut reader: Box<dyn needletail::FastxReader>,
    estimated_unique: usize,
    show_progress: bool,
) -> Result<(DualSeqCounts, u64)> {
    if show_progress {
        eprint!("      {:<10} ... ", "counting");
        std::io::Write::flush(&mut std::io::stderr()).ok();
    }

    let mut counts = DualSeqCounts::with_capacity(estimated_unique, estimated_unique / 10);
    let mut total_records = 0u64;

    while let Some(record) = reader.next() {
        let record = record.context("Failed to read record")?;
        counts.insert(record.seq());
        total_records += 1;
    }

    if show_progress {
        eprintln!("{} records", format_count(total_records));
    }

    Ok((counts, total_records))
}

#[cfg(feature = "cli")]
pub fn prepare_records(counts: DualSeqCounts) -> Vec<SequenceRecord> {
    let total_unique = counts.packed.len() + counts.long.len();
    let mut records: Vec<SequenceRecord> = Vec::with_capacity(total_unique);

    for (key, count) in counts.packed {
        records.push(SequenceRecord {
            sequence: SequenceData::Packed(key),
            count,
        });
    }

    for (seq, count) in counts.long {
        records.push(SequenceRecord {
            sequence: SequenceData::Raw(seq),
            count,
        });
    }

    records.sort_unstable_by(|a, b| b.count.cmp(&a.count));
    records
}
