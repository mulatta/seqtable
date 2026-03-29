use anyhow::{Context, Result};
use arrow::array::{Float64Array, UInt64Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use clap::ValueEnum;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

// Increased buffer size for better I/O performance
const WRITE_BUFFER_SIZE: usize = 512 * 1024; // 512KB

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormat {
    Parquet,
    Csv,
    Tsv,
}

impl OutputFormat {
    pub fn extension(&self) -> &str {
        match self {
            OutputFormat::Parquet => "parquet",
            OutputFormat::Csv => "csv",
            OutputFormat::Tsv => "tsv",
        }
    }
}

pub struct SequenceRecord {
    pub sequence: SequenceData,
    pub count: u64,
}

/// Holds sequence data — either 2-bit packed (no heap allocation) or raw bytes.
pub enum SequenceData {
    Packed(crate::PackedDna),
    Raw(Vec<u8>),
}

impl SequenceRecord {
    /// Write sequence as &str into a reusable buffer (avoids allocation).
    pub fn sequence_str<'a>(&'a self, buf: &'a mut Vec<u8>) -> &'a str {
        match &self.sequence {
            SequenceData::Packed(p) => {
                buf.clear();
                crate::unpack_dna_into(p, buf);
                // ACGT-only, always valid UTF-8
                unsafe { std::str::from_utf8_unchecked(buf) }
            }
            SequenceData::Raw(v) => {
                std::str::from_utf8(v).expect("FASTQ sequence is not valid UTF-8")
            }
        }
    }
}

pub fn save_output(
    records: &[SequenceRecord],
    output_path: &Path,
    format: &OutputFormat,
    compression: parquet::basic::Compression,
    total_reads: u64,
    include_rpm: bool,
) -> Result<()> {
    match format {
        OutputFormat::Parquet => {
            save_parquet(records, output_path, compression, total_reads, include_rpm)?
        }
        OutputFormat::Csv => save_csv(records, output_path, b',', total_reads, include_rpm)?,
        OutputFormat::Tsv => save_csv(records, output_path, b'\t', total_reads, include_rpm)?,
    }
    Ok(())
}

pub fn save_parquet(
    records: &[SequenceRecord],
    output_path: &Path,
    compression: parquet::basic::Compression,
    total_reads: u64,
    include_rpm: bool,
) -> Result<()> {
    let mut fields = vec![
        Field::new("sequence", DataType::Utf8, false),
        Field::new("count", DataType::UInt64, false),
    ];

    if include_rpm {
        fields.push(Field::new("rpm", DataType::Float64, false));
    }

    let schema = Arc::new(Schema::new(fields));

    let capacity = records.len();
    let mut counts = Vec::with_capacity(capacity);
    let mut buf = Vec::with_capacity(160);

    // Build StringArray via builder to avoid intermediate String allocations
    let mut seq_builder = arrow::array::StringBuilder::with_capacity(capacity, capacity * 151);
    for record in records {
        let seq = record.sequence_str(&mut buf);
        seq_builder.append_value(seq);
        counts.push(record.count);
    }
    let seq_array = seq_builder.finish();
    let count_array = UInt64Array::from(counts);

    let mut arrays: Vec<Arc<dyn arrow::array::Array>> =
        vec![Arc::new(seq_array), Arc::new(count_array)];

    if include_rpm && total_reads > 0 {
        let rpm_scale = 1_000_000.0 / total_reads as f64;
        let rpm_values: Vec<f64> = records.iter().map(|r| r.count as f64 * rpm_scale).collect();
        arrays.push(Arc::new(Float64Array::from(rpm_values)));
    }

    let batch =
        RecordBatch::try_new(schema.clone(), arrays).context("Failed to create RecordBatch")?;

    let file = File::create(output_path)
        .with_context(|| format!("Failed to create file: {}", output_path.display()))?;

    let props = WriterProperties::builder()
        .set_compression(compression)
        .build();

    let mut writer =
        ArrowWriter::try_new(file, schema, Some(props)).context("Failed to create ArrowWriter")?;

    writer.write(&batch).context("Failed to write data")?;
    writer.close().context("Failed to close file")?;

    Ok(())
}

pub fn save_csv(
    records: &[SequenceRecord],
    output_path: &Path,
    delimiter: u8,
    total_reads: u64,
    include_rpm: bool,
) -> Result<()> {
    let file = File::create(output_path)
        .with_context(|| format!("Failed to create file: {}", output_path.display()))?;

    let mut csv_writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .buffer_capacity(WRITE_BUFFER_SIZE)
        .from_writer(file);

    let include_rpm = include_rpm && total_reads > 0;
    if include_rpm {
        csv_writer.write_record(["sequence", "count", "rpm"])?;
    } else {
        csv_writer.write_record(["sequence", "count"])?;
    }

    use std::fmt::Write as _;
    let mut count_buf = String::with_capacity(16);
    let mut rpm_buf = String::with_capacity(16);
    let rpm_scale = if include_rpm {
        1_000_000.0 / total_reads as f64
    } else {
        0.0
    };

    let mut seq_buf = Vec::with_capacity(160);
    for record in records {
        let seq = record.sequence_str(&mut seq_buf);
        count_buf.clear();
        write!(count_buf, "{}", record.count).unwrap();
        if include_rpm {
            rpm_buf.clear();
            write!(rpm_buf, "{:.2}", record.count as f64 * rpm_scale).unwrap();
            csv_writer.write_record([seq, &count_buf, &rpm_buf])?;
        } else {
            csv_writer.write_record([seq, &count_buf])?;
        }
    }

    csv_writer.flush()?;
    Ok(())
}
