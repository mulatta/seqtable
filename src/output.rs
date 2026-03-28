use anyhow::{Context, Result};
use arrow::array::{Float64Array, UInt64Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use clap::ValueEnum;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::io::BufWriter;
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
    pub rpm: Option<f64>,
}

/// Holds sequence data — either 2-bit packed (no heap allocation) or raw bytes.
pub enum SequenceData {
    Packed(crate::PackedDna),
    Raw(Vec<u8>),
}

impl SequenceRecord {
    /// Unpack into a new String (for parquet batch building).
    pub fn sequence_string(&self) -> String {
        match &self.sequence {
            SequenceData::Packed(p) => {
                // ACGT bytes are valid ASCII/UTF-8
                unsafe { String::from_utf8_unchecked(crate::unpack_dna(p)) }
            }
            SequenceData::Raw(v) => std::str::from_utf8(v)
                .expect("FASTQ sequence is not valid UTF-8")
                .to_owned(),
        }
    }

    /// Write sequence as &str into a reusable buffer (for CSV row-by-row writing).
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
) -> Result<()> {
    match format {
        OutputFormat::Parquet => save_parquet(records, output_path, compression)?,
        OutputFormat::Csv => save_csv(records, output_path, b',')?,
        OutputFormat::Tsv => save_csv(records, output_path, b'\t')?,
    }
    Ok(())
}

pub fn save_parquet(
    records: &[SequenceRecord],
    output_path: &Path,
    compression: parquet::basic::Compression,
) -> Result<()> {
    let mut fields = vec![
        Field::new("sequence", DataType::Utf8, false),
        Field::new("count", DataType::UInt64, false),
    ];

    if records.first().and_then(|r| r.rpm).is_some() {
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

    if records.first().and_then(|r| r.rpm).is_some() {
        let rpm_values: Vec<f64> = records.iter().map(|r| r.rpm.unwrap()).collect();
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

pub fn save_csv(records: &[SequenceRecord], output_path: &Path, delimiter: u8) -> Result<()> {
    let file = File::create(output_path)
        .with_context(|| format!("Failed to create file: {}", output_path.display()))?;

    let writer = BufWriter::with_capacity(WRITE_BUFFER_SIZE, file);

    let mut csv_writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .buffer_capacity(WRITE_BUFFER_SIZE)
        .from_writer(writer);

    let has_rpm = records.first().and_then(|r| r.rpm).is_some();
    if has_rpm {
        csv_writer.write_record(["sequence", "count", "rpm"])?;
    } else {
        csv_writer.write_record(["sequence", "count"])?;
    }

    use std::fmt::Write as _;
    let mut count_buf = String::with_capacity(16);
    let mut rpm_buf = String::with_capacity(16);

    let mut seq_buf = Vec::with_capacity(160);
    for record in records {
        let seq = record.sequence_str(&mut seq_buf);
        count_buf.clear();
        write!(count_buf, "{}", record.count).unwrap();
        if let Some(rpm) = record.rpm {
            rpm_buf.clear();
            write!(rpm_buf, "{:.2}", rpm).unwrap();
            csv_writer.write_record([seq, &count_buf, &rpm_buf])?;
        } else {
            csv_writer.write_record([seq, &count_buf])?;
        }
    }

    csv_writer.flush()?;
    Ok(())
}
