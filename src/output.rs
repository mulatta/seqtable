use crate::Args;
use anyhow::{Context, Result};
use arrow::array::{Float64Array, LargeStringArray, UInt64Array};
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
    pub sequence: String,
    pub count: u64,
    pub rpm: Option<f64>,
}

pub fn save_output(records: &[SequenceRecord], output_path: &Path, args: &Args) -> Result<()> {
    if !args.quiet {
        print!(
            "   💾 Saving to {}...",
            args.format.extension().to_uppercase()
        );
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }

    match args.format {
        OutputFormat::Parquet => save_parquet(records, output_path, &args.compression)?,
        OutputFormat::Csv => save_csv(records, output_path, b',')?,
        OutputFormat::Tsv => save_csv(records, output_path, b'\t')?,
    }

    if !args.quiet {
        println!(" Done!");
    }
    Ok(())
}

fn save_parquet(records: &[SequenceRecord], output_path: &Path, compression: &str) -> Result<()> {
    // Define schema
    let mut fields = vec![
        Field::new("sequence", DataType::LargeUtf8, false),
        Field::new("count", DataType::UInt64, false),
    ];

    if records.first().and_then(|r| r.rpm).is_some() {
        fields.push(Field::new("rpm", DataType::Float64, false));
    }

    let schema = Arc::new(Schema::new(fields));

    // Pre-allocate with capacity
    let capacity = records.len();
    let mut sequences = Vec::with_capacity(capacity);
    let mut counts = Vec::with_capacity(capacity);

    for record in records {
        sequences.push(record.sequence.as_str());
        counts.push(record.count);
    }

    let seq_array = LargeStringArray::from(sequences);
    let count_array = UInt64Array::from(counts);

    // Build arrays
    let mut arrays: Vec<Arc<dyn arrow::array::Array>> =
        vec![Arc::new(seq_array), Arc::new(count_array)];

    // Add RPM if present
    if records.first().and_then(|r| r.rpm).is_some() {
        let rpm_values: Vec<f64> = records.iter().map(|r| r.rpm.unwrap()).collect();
        arrays.push(Arc::new(Float64Array::from(rpm_values)));
    }

    // Create RecordBatch
    let batch =
        RecordBatch::try_new(schema.clone(), arrays).context("Failed to create RecordBatch")?;

    // Configure Parquet writer
    let file = File::create(output_path)
        .with_context(|| format!("Failed to create file: {}", output_path.display()))?;

    let compression = match compression.to_lowercase().as_str() {
        "snappy" => parquet::basic::Compression::SNAPPY,
        "gzip" => parquet::basic::Compression::GZIP(parquet::basic::GzipLevel::default()),
        "brotli" => parquet::basic::Compression::BROTLI(parquet::basic::BrotliLevel::default()),
        "zstd" => parquet::basic::Compression::ZSTD(parquet::basic::ZstdLevel::default()),
        "none" => parquet::basic::Compression::UNCOMPRESSED,
        _ => parquet::basic::Compression::SNAPPY,
    };

    let props = WriterProperties::builder()
        .set_compression(compression)
        .build();

    let mut writer =
        ArrowWriter::try_new(file, schema, Some(props)).context("Failed to create ArrowWriter")?;

    writer.write(&batch).context("Failed to write data")?;
    writer.close().context("Failed to close file")?;

    Ok(())
}

fn save_csv(records: &[SequenceRecord], output_path: &Path, delimiter: u8) -> Result<()> {
    let file = File::create(output_path)
        .with_context(|| format!("Failed to create file: {}", output_path.display()))?;

    // Use larger buffer for better I/O performance
    let writer = BufWriter::with_capacity(WRITE_BUFFER_SIZE, file);

    let mut csv_writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .buffer_capacity(WRITE_BUFFER_SIZE)
        .from_writer(writer);

    // Write header
    let has_rpm = records.first().and_then(|r| r.rpm).is_some();
    if has_rpm {
        csv_writer.write_record(["sequence", "count", "rpm"])?;
    } else {
        csv_writer.write_record(["sequence", "count"])?;
    }

    // Write data
    for record in records {
        if let Some(rpm) = record.rpm {
            csv_writer.write_record([
                record.sequence.as_str(),
                &record.count.to_string(),
                &format!("{:.2}", rpm),
            ])?;
        } else {
            csv_writer.write_record([record.sequence.as_str(), &record.count.to_string()])?;
        }
    }

    csv_writer.flush()?;
    Ok(())
}
