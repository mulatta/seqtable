use ahash::AHashMap;
use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, Throughput,
};
use seqtable::output::{save_csv, save_parquet, SequenceRecord};
use seqtable::{count_sequences, count_sequences_sequential, prepare_records};
use std::io::Write;
use tempfile::NamedTempFile;

// --- Test data ---

const LOW_UNIQ: &[(&str, usize)] = &[
    ("AAGCCCAATAAACCACTCTGAC", 41),
    ("TGGCCGAATAGGGATATAGGCA", 24),
    ("ACGACATGTGCGGCGACCCTTG", 15),
    ("CGACAGTGACGCTTTCGCCGTT", 11),
    ("GCCTAAACCTATTTGAAGGAGT", 9),
];

fn create_fastq(records: &[(&str, usize)]) -> NamedTempFile {
    let mut f = NamedTempFile::with_suffix(".fastq").unwrap();
    let mut read_id = 0u64;
    for (seq, count) in records {
        for _ in 0..*count {
            let qual: String = std::iter::repeat_n('I', seq.len()).collect();
            writeln!(f, "@read_{read_id}\n{seq}\n+\n{qual}").unwrap();
            read_id += 1;
        }
    }
    f.flush().unwrap();
    f
}

fn generate_sequences(n_unique: usize, reads_per_seq: usize) -> Vec<(String, usize)> {
    (0..n_unique)
        .map(|i| {
            let bases = ['A', 'C', 'G', 'T'];
            let seq: String = (0..22)
                .map(|j| bases[(i * 7 + j * 3) % 4])
                .collect();
            (seq, reads_per_seq)
        })
        .collect()
}

fn create_scaled_fastq(n_unique: usize, reads_per_seq: usize) -> NamedTempFile {
    let data = generate_sequences(n_unique, reads_per_seq);
    let refs: Vec<(&str, usize)> = data.iter().map(|(s, c)| (s.as_str(), *c)).collect();
    create_fastq(&refs)
}

fn make_records(n: usize) -> Vec<SequenceRecord> {
    (0..n)
        .map(|i| {
            let bases = ['A', 'C', 'G', 'T'];
            SequenceRecord {
                sequence: (0..22).map(|j| bases[(i * 7 + j * 3) % 4]).collect(),
                count: (n - i) as u64,
                rpm: None,
            }
        })
        .collect()
}

fn make_counts(n: usize) -> (AHashMap<String, u64>, u64) {
    let mut map = AHashMap::with_capacity(n);
    let mut total = 0u64;
    let bases = ['A', 'C', 'G', 'T'];
    for i in 0..n {
        let seq: String = (0..22).map(|j| bases[(i * 7 + j * 3) % 4]).collect();
        let count = (n - i) as u64;
        map.insert(seq, count);
        total += count;
    }
    (map, total)
}

// --- Benchmarks ---

fn bench_count_sequences(c: &mut Criterion) {
    let mut group = c.benchmark_group("count_sequences");

    // Small: 100 reads, 5 unique
    let small = create_fastq(LOW_UNIQ);
    group.throughput(Throughput::Elements(100));
    group.bench_function("100r_5u_seq", |b| {
        b.iter(|| count_sequences_sequential(small.path(), false).unwrap())
    });
    group.bench_function("100r_5u_par", |b| {
        b.iter(|| count_sequences(small.path(), 0, false).unwrap())
    });

    // Medium: 1000 reads, 50 unique
    let medium = create_scaled_fastq(50, 20);
    group.throughput(Throughput::Elements(1000));
    group.bench_function("1kr_50u_seq", |b| {
        b.iter(|| count_sequences_sequential(medium.path(), false).unwrap())
    });
    group.bench_function("1kr_50u_par", |b| {
        b.iter(|| count_sequences(medium.path(), 500, false).unwrap())
    });

    // Large: 10000 reads, 200 unique
    let large = create_scaled_fastq(200, 50);
    group.throughput(Throughput::Elements(10_000));
    for chunk in [0, 1000, 5000] {
        group.bench_with_input(
            BenchmarkId::new("10kr_200u", format!("chunk_{chunk}")),
            &chunk,
            |b, &cs| {
                b.iter(|| count_sequences(large.path(), cs, false).unwrap())
            },
        );
    }

    group.finish();
}

fn bench_prepare_records(c: &mut Criterion) {
    let mut group = c.benchmark_group("prepare_records");

    for &n in &[100, 1000, 10_000] {
        let (counts, total) = make_counts(n);
        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(
            BenchmarkId::new("no_rpm", n),
            &(counts.clone(), total),
            |b, (c, t)| b.iter(|| prepare_records(c.clone(), *t, false)),
        );
        group.bench_with_input(
            BenchmarkId::new("with_rpm", n),
            &(counts.clone(), total),
            |b, (c, t)| b.iter(|| prepare_records(c.clone(), *t, true)),
        );
    }

    group.finish();
}

fn bench_save_csv(c: &mut Criterion) {
    let mut group = c.benchmark_group("save_csv");

    for &n in &[100, 1000, 10_000] {
        let records = make_records(n);
        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(BenchmarkId::from_parameter(n), &records, |b, recs| {
            b.iter(|| {
                let tmp = NamedTempFile::new().unwrap();
                save_csv(recs, tmp.path(), b',').unwrap()
            })
        });
    }

    group.finish();
}

fn bench_save_parquet(c: &mut Criterion) {
    let mut group = c.benchmark_group("save_parquet");
    let records = make_records(1000);
    group.throughput(Throughput::Elements(1000));

    let compressions = [
        (
            "zstd",
            parquet::basic::Compression::ZSTD(Default::default()),
        ),
        ("snappy", parquet::basic::Compression::SNAPPY),
        ("none", parquet::basic::Compression::UNCOMPRESSED),
    ];

    for (name, comp) in compressions {
        group.bench_with_input(BenchmarkId::new(name, 1000), &comp, |b, compression| {
            b.iter(|| {
                let tmp = NamedTempFile::new().unwrap();
                save_parquet(&records, tmp.path(), *compression).unwrap()
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_count_sequences,
    bench_prepare_records,
    bench_save_csv,
    bench_save_parquet,
);
criterion_main!(benches);
