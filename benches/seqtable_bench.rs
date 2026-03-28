use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use seqtable::output::{save_csv, save_parquet};
use seqtable::{SequenceRecord, count_sequences, count_sequences_sequential, prepare_records};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

const FIXTURE_DIR: &str = "tests/fixtures";
const READS: u64 = 10_000;

// bn_<len>_<uniq>_10000.fastq
struct Fixture {
    name: &'static str,
    path: PathBuf,
}

fn bench_fixtures() -> Vec<Fixture> {
    let files = [
        ("short_low", "bn_short_low_10000.fastq"),
        ("short_mid", "bn_short_mid_10000.fastq"),
        ("short_high", "bn_short_high_10000.fastq"),
        ("amp_low", "bn_amp_low_10000.fastq"),
        ("amp_mid", "bn_amp_mid_10000.fastq"),
        ("amp_high", "bn_amp_high_10000.fastq"),
    ];

    let mut fixtures = Vec::new();
    for (name, file) in files {
        let path = Path::new(FIXTURE_DIR).join(file);
        if path.exists() {
            fixtures.push(Fixture { name, path });
        }
    }
    fixtures
}

fn bench_fixtures_gz() -> Vec<Fixture> {
    let files = [
        ("short_low_gz", "bn_short_low_10000.fastq.gz"),
        ("short_mid_gz", "bn_short_mid_10000.fastq.gz"),
        ("short_high_gz", "bn_short_high_10000.fastq.gz"),
        ("amp_low_gz", "bn_amp_low_10000.fastq.gz"),
        ("amp_mid_gz", "bn_amp_mid_10000.fastq.gz"),
        ("amp_high_gz", "bn_amp_high_10000.fastq.gz"),
    ];

    let mut fixtures = Vec::new();
    for (name, file) in files {
        let path = Path::new(FIXTURE_DIR).join(file);
        if path.exists() {
            fixtures.push(Fixture { name, path });
        }
    }
    fixtures
}

fn load_records_from_fixture(path: &Path) -> Vec<SequenceRecord> {
    let (counts, total) = count_sequences_sequential(path, false).unwrap();
    prepare_records(counts)
}

// --- Benchmarks ---

fn bench_count_sequences(c: &mut Criterion) {
    let fixtures = bench_fixtures();
    if fixtures.is_empty() {
        eprintln!(
            "bench fixtures not found, run: cargo run --example generate_fixtures --release -- --size bench"
        );
        return;
    }

    let mut group = c.benchmark_group("count_sequences");
    group.throughput(Throughput::Elements(READS));

    for f in &fixtures {
        group.bench_with_input(
            BenchmarkId::new("sequential", f.name),
            &f.path,
            |b, path| b.iter(|| count_sequences_sequential(path, false).unwrap()),
        );

        for chunk in [0, 1000, 5000] {
            group.bench_with_input(
                BenchmarkId::new(format!("parallel/chunk_{chunk}"), f.name),
                &f.path,
                |b, path| b.iter(|| count_sequences(path, chunk, false).unwrap()),
            );
        }
    }

    group.finish();
}

fn bench_count_sequences_gz(c: &mut Criterion) {
    let fixtures = bench_fixtures_gz();
    if fixtures.is_empty() {
        eprintln!("gz bench fixtures not found, run: gzip -k tests/fixtures/bn_*.fastq");
        return;
    }

    let mut group = c.benchmark_group("count_sequences_gz");
    group.throughput(Throughput::Elements(READS));

    for f in &fixtures {
        group.bench_with_input(
            BenchmarkId::new("sequential", f.name),
            &f.path,
            |b, path| b.iter(|| count_sequences_sequential(path, false).unwrap()),
        );

        group.bench_with_input(
            BenchmarkId::new("parallel/chunk_0", f.name),
            &f.path,
            |b, path| b.iter(|| count_sequences(path, 0, false).unwrap()),
        );
    }

    group.finish();
}

fn bench_prepare_records(c: &mut Criterion) {
    let fixtures = bench_fixtures();
    if fixtures.is_empty() {
        return;
    }

    let mut group = c.benchmark_group("prepare_records");

    for f in &fixtures {
        let (counts, _total) = count_sequences_sequential(&f.path, false).unwrap();
        let n = counts.len() as u64;
        group.throughput(Throughput::Elements(n));

        group.bench_with_input(BenchmarkId::from_parameter(f.name), &counts, |b, c| {
            b.iter(|| prepare_records(c.clone()))
        });
    }

    group.finish();
}

fn bench_save_csv(c: &mut Criterion) {
    let fixtures = bench_fixtures();
    if fixtures.is_empty() {
        return;
    }

    let mut group = c.benchmark_group("save_csv");

    for f in &fixtures {
        let records = load_records_from_fixture(&f.path);
        group.throughput(Throughput::Elements(records.len() as u64));

        group.bench_with_input(BenchmarkId::from_parameter(f.name), &records, |b, recs| {
            b.iter(|| {
                let tmp = NamedTempFile::new().unwrap();
                save_csv(recs, tmp.path(), b',', 10_000, false).unwrap()
            })
        });
    }

    group.finish();
}

fn bench_save_parquet(c: &mut Criterion) {
    let fixtures = bench_fixtures();
    if fixtures.is_empty() {
        return;
    }

    let mut group = c.benchmark_group("save_parquet");

    let compressions = [
        (
            "zstd",
            parquet::basic::Compression::ZSTD(Default::default()),
        ),
        ("snappy", parquet::basic::Compression::SNAPPY),
        ("none", parquet::basic::Compression::UNCOMPRESSED),
    ];

    // Use mid-uniqueness short fixture as representative
    let mid = fixtures.iter().find(|f| f.name == "short_mid");
    let Some(f) = mid else { return };

    let records = load_records_from_fixture(&f.path);
    group.throughput(Throughput::Elements(records.len() as u64));

    for (name, comp) in compressions {
        group.bench_with_input(BenchmarkId::new(name, f.name), &comp, |b, compression| {
            b.iter(|| {
                let tmp = NamedTempFile::new().unwrap();
                save_parquet(&records, tmp.path(), *compression, 10_000, false).unwrap()
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_count_sequences,
    bench_count_sequences_gz,
    bench_prepare_records,
    bench_save_csv,
    bench_save_parquet,
);
criterion_main!(benches);
