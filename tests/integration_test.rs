use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn seqtable_bin() -> PathBuf {
    std::env::current_exe()
        .expect("current exe")
        .parent()
        .expect("parent")
        .parent()
        .expect("parent")
        .join("seqtable")
}

fn run_seqtable(args: &[&str]) -> (String, String, bool) {
    let output = Command::new(seqtable_bin())
        .args(args)
        .output()
        .expect("failed to execute seqtable");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

fn parse_csv_counts(csv: &str) -> HashMap<String, u64> {
    csv.lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.splitn(3, ',');
            let seq = fields.next()?.to_string();
            let count: u64 = fields.next()?.parse().ok()?;
            Some((seq, count))
        })
        .collect()
}

fn with_temp_dir<F: FnOnce(&Path)>(f: F) {
    let dir = tempfile::tempdir().expect("tempdir");
    f(dir.path());
}

/// Create a FASTQ file with known content (deterministic, self-contained)
fn write_fastq(path: &Path, records: &[(&str, usize)]) {
    let mut f = std::fs::File::create(path).expect("create fastq");
    let mut read_id = 0;
    for (seq, count) in records {
        for _ in 0..*count {
            let qual: String = std::iter::repeat_n('I', seq.len()).collect();
            writeln!(f, "@read_{read_id}\n{seq}\n+\n{qual}").expect("write");
            read_id += 1;
        }
    }
}

// Known test data: 5 sequences, 100 total reads
const LOW_UNIQ: &[(&str, usize)] = &[
    ("AAGCCCAATAAACCACTCTGAC", 41),
    ("TGGCCGAATAGGGATATAGGCA", 24),
    ("ACGACATGTGCGGCGACCCTTG", 15),
    ("CGACAGTGACGCTTTCGCCGTT", 11),
    ("GCCTAAACCTATTTGAAGGAGT", 9),
];

// Known test data: variable length sequences
const AMPLICON: &[(&str, usize)] = &[
    (
        "AAGCCCAATAAACCACTCTGACTGGCCGAATAGGGATATAGGCAACGACATGTGCGGCGAC",
        30,
    ),
    ("TGGCCGA", 25),
    (
        "ACGACATGTGCGGCGACCCTTGCGACAGTGACGCTTTCGCCGTTGCCTAAACCTATTTGAAGGAGT",
        20,
    ),
    ("CGACAGTGACGCTTTCGCCGTTGCCTAAACCTATTTG", 15),
    ("GCCTAAACCTATTTGAAGGAGTCTAGCAGCCGCAGT", 10),
];

// --- Correctness tests ---

#[test]
fn test_exact_counts() {
    with_temp_dir(|dir| {
        let input = dir.join("test.fastq");
        write_fastq(&input, LOW_UNIQ);

        let (_, _, ok) = run_seqtable(&[
            input.to_str().unwrap(),
            "-o",
            dir.to_str().unwrap(),
            "-f",
            "csv",
            "-q",
        ]);
        assert!(ok);

        let csv = std::fs::read_to_string(dir.join("test.csv")).expect("read csv");
        let counts = parse_csv_counts(&csv);

        assert_eq!(counts.len(), 5);
        assert_eq!(counts["AAGCCCAATAAACCACTCTGAC"], 41);
        assert_eq!(counts["TGGCCGAATAGGGATATAGGCA"], 24);
        assert_eq!(counts["ACGACATGTGCGGCGACCCTTG"], 15);
        assert_eq!(counts["CGACAGTGACGCTTTCGCCGTT"], 11);
        assert_eq!(counts["GCCTAAACCTATTTGAAGGAGT"], 9);

        let total: u64 = counts.values().sum();
        assert_eq!(total, 100);
    });
}

#[test]
fn test_rpm_calculation() {
    with_temp_dir(|dir| {
        let input = dir.join("test.fastq");
        write_fastq(&input, LOW_UNIQ);

        let (_, _, ok) = run_seqtable(&[
            input.to_str().unwrap(),
            "-o",
            dir.to_str().unwrap(),
            "-f",
            "csv",
            "-q",
            "--rpm",
        ]);
        assert!(ok);

        let csv = std::fs::read_to_string(dir.join("test.csv")).expect("read csv");
        let lines: Vec<&str> = csv.lines().collect();

        assert_eq!(lines[0], "sequence,count,rpm");

        // Top sequence: 41/100 * 1_000_000 = 410_000
        let fields: Vec<&str> = lines[1].split(',').collect();
        assert_eq!(fields[0], "AAGCCCAATAAACCACTCTGAC");
        let rpm: f64 = fields[2].parse().expect("parse rpm");
        assert!((rpm - 410_000.0).abs() < 0.01);
    });
}

#[test]
fn test_sorted_by_count_desc() {
    with_temp_dir(|dir| {
        let input = dir.join("test.fastq");
        write_fastq(&input, LOW_UNIQ);

        let (_, _, ok) = run_seqtable(&[
            input.to_str().unwrap(),
            "-o",
            dir.to_str().unwrap(),
            "-f",
            "csv",
            "-q",
        ]);
        assert!(ok);

        let csv = std::fs::read_to_string(dir.join("test.csv")).expect("read csv");
        let counts: Vec<u64> = csv
            .lines()
            .skip(1)
            .map(|l| l.split(',').nth(1).unwrap().parse().unwrap())
            .collect();

        for w in counts.windows(2) {
            assert!(w[0] >= w[1], "should be sorted descending");
        }
    });
}

#[test]
fn test_amplicon_variable_length() {
    with_temp_dir(|dir| {
        let input = dir.join("test.fastq");
        write_fastq(&input, AMPLICON);

        let (_, _, ok) = run_seqtable(&[
            input.to_str().unwrap(),
            "-o",
            dir.to_str().unwrap(),
            "-f",
            "csv",
            "-q",
        ]);
        assert!(ok);

        let csv = std::fs::read_to_string(dir.join("test.csv")).expect("read csv");
        let counts = parse_csv_counts(&csv);

        assert_eq!(counts.len(), 5);
        let total: u64 = counts.values().sum();
        assert_eq!(total, 100);

        let lengths: std::collections::HashSet<usize> = counts.keys().map(|s| s.len()).collect();
        assert!(lengths.len() > 1, "amplicon should have variable lengths");
    });
}

#[test]
fn test_tsv_output() {
    with_temp_dir(|dir| {
        let input = dir.join("test.fastq");
        write_fastq(&input, LOW_UNIQ);

        let (_, _, ok) = run_seqtable(&[
            input.to_str().unwrap(),
            "-o",
            dir.to_str().unwrap(),
            "-f",
            "tsv",
            "-q",
        ]);
        assert!(ok);

        let tsv = std::fs::read_to_string(dir.join("test.tsv")).expect("read tsv");
        assert!(tsv.lines().nth(1).unwrap().contains('\t'));
    });
}

#[test]
fn test_parquet_output() {
    with_temp_dir(|dir| {
        let input = dir.join("test.fastq");
        write_fastq(&input, LOW_UNIQ);

        let (_, _, ok) =
            run_seqtable(&[input.to_str().unwrap(), "-o", dir.to_str().unwrap(), "-q"]);
        assert!(ok);
        assert!(dir.join("test.parquet").exists());
    });
}

#[test]
fn test_reject_fasta() {
    with_temp_dir(|dir| {
        let fasta = dir.join("test.fasta");
        std::fs::write(&fasta, ">seq1\nACGT\n").unwrap();

        let (_, stderr, ok) =
            run_seqtable(&[fasta.to_str().unwrap(), "-o", dir.to_str().unwrap(), "-q"]);
        assert!(!ok);
        assert!(stderr.contains("unsupported file format"));
    });
}

#[test]
fn test_multiple_files() {
    with_temp_dir(|dir| {
        let input1 = dir.join("a.fastq");
        let input2 = dir.join("b.fastq");
        write_fastq(&input1, LOW_UNIQ);
        write_fastq(&input2, AMPLICON);

        let (_, _, ok) = run_seqtable(&[
            input1.to_str().unwrap(),
            input2.to_str().unwrap(),
            "-o",
            dir.to_str().unwrap(),
            "-f",
            "csv",
            "-q",
        ]);
        assert!(ok);
        assert!(dir.join("a.csv").exists());
        assert!(dir.join("b.csv").exists());
    });
}

#[test]
fn test_stderr_not_stdout() {
    with_temp_dir(|dir| {
        let input = dir.join("test.fastq");
        write_fastq(&input, LOW_UNIQ);

        let (stdout, stderr, ok) = run_seqtable(&[
            input.to_str().unwrap(),
            "-o",
            dir.to_str().unwrap(),
            "-f",
            "csv",
        ]);
        assert!(ok);
        assert!(stdout.is_empty(), "stdout should be empty, got: {stdout}");
        assert!(stderr.contains("seqtable"));
    });
}
