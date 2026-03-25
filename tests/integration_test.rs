use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn seqtable_bin() -> PathBuf {
    // Find the binary built by cargo test
    let mut path = std::env::current_exe()
        .expect("failed to get current exe")
        .parent()
        .expect("failed to get parent")
        .parent()
        .expect("failed to get parent")
        .to_path_buf();
    path.push("seqtable");
    path
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn run_seqtable(args: &[&str]) -> (String, String, bool) {
    let output = Command::new(seqtable_bin())
        .args(args)
        .output()
        .expect("failed to execute seqtable");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (stdout, stderr, output.status.success())
}

fn parse_csv_counts(csv_content: &str) -> HashMap<String, u64> {
    let mut counts = HashMap::new();
    for line in csv_content.lines().skip(1) {
        // skip header
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() >= 2 {
            let seq = fields[0].to_string();
            let count: u64 = fields[1].parse().expect("invalid count");
            counts.insert(seq, count);
        }
    }
    counts
}

fn with_temp_dir<F: FnOnce(&Path)>(f: F) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    f(dir.path());
}

// --- Correctness tests ---

#[test]
fn test_small_low_uniq_counts() {
    with_temp_dir(|dir| {
        let input = fixture_path("small_low_uniq.fastq");
        let (_, _, ok) = run_seqtable(&[
            input.to_str().unwrap(),
            "-o",
            dir.to_str().unwrap(),
            "-f",
            "csv",
            "-q",
        ]);
        assert!(ok, "seqtable should succeed");

        let csv = std::fs::read_to_string(dir.join("small_low_uniq.csv")).expect("read csv");
        let counts = parse_csv_counts(&csv);

        // Expected counts (deterministic fixture, seed=42)
        assert_eq!(counts.len(), 5);
        assert_eq!(counts["AAGCCCAATAAACCACTCTGAC"], 41);
        assert_eq!(counts["TGGCCGAATAGGGATATAGGCA"], 24);
        assert_eq!(counts["ACGACATGTGCGGCGACCCTTG"], 15);
        assert_eq!(counts["CGACAGTGACGCTTTCGCCGTT"], 11);
        assert_eq!(counts["GCCTAAACCTATTTGAAGGAGT"], 9);

        // Total reads
        let total: u64 = counts.values().sum();
        assert_eq!(total, 100);
    });
}

#[test]
fn test_small_high_uniq_counts() {
    with_temp_dir(|dir| {
        let input = fixture_path("small_high_uniq.fastq");
        let (_, _, ok) = run_seqtable(&[
            input.to_str().unwrap(),
            "-o",
            dir.to_str().unwrap(),
            "-f",
            "csv",
            "-q",
        ]);
        assert!(ok);

        let csv = std::fs::read_to_string(dir.join("small_high_uniq.csv")).expect("read csv");
        let counts = parse_csv_counts(&csv);

        assert_eq!(counts.len(), 35);
        assert_eq!(counts["AAGCCCAATAAACCACTCTGAC"], 14);
        let total: u64 = counts.values().sum();
        assert_eq!(total, 100);
    });
}

#[test]
fn test_rpm_calculation() {
    with_temp_dir(|dir| {
        let input = fixture_path("small_low_uniq.fastq");
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

        let csv = std::fs::read_to_string(dir.join("small_low_uniq.csv")).expect("read csv");
        let lines: Vec<&str> = csv.lines().collect();

        // Header should have rpm column
        assert_eq!(lines[0], "sequence,count,rpm");

        // Check RPM for top sequence: 41/100 * 1_000_000 = 410_000
        let fields: Vec<&str> = lines[1].split(',').collect();
        assert_eq!(fields[0], "AAGCCCAATAAACCACTCTGAC");
        let rpm: f64 = fields[2].parse().expect("parse rpm");
        assert!((rpm - 410_000.0).abs() < 0.01);
    });
}

#[test]
fn test_csv_output_sorted_by_count_desc() {
    with_temp_dir(|dir| {
        let input = fixture_path("small_low_uniq.fastq");
        let (_, _, ok) = run_seqtable(&[
            input.to_str().unwrap(),
            "-o",
            dir.to_str().unwrap(),
            "-f",
            "csv",
            "-q",
        ]);
        assert!(ok);

        let csv = std::fs::read_to_string(dir.join("small_low_uniq.csv")).expect("read csv");
        let counts: Vec<u64> = csv
            .lines()
            .skip(1)
            .map(|l| l.split(',').nth(1).unwrap().parse::<u64>().unwrap())
            .collect();

        // Verify descending order
        for w in counts.windows(2) {
            assert!(w[0] >= w[1], "counts should be sorted descending");
        }
    });
}

#[test]
fn test_tsv_output() {
    with_temp_dir(|dir| {
        let input = fixture_path("small_low_uniq.fastq");
        let (_, _, ok) = run_seqtable(&[
            input.to_str().unwrap(),
            "-o",
            dir.to_str().unwrap(),
            "-f",
            "tsv",
            "-q",
        ]);
        assert!(ok);

        let tsv = std::fs::read_to_string(dir.join("small_low_uniq.tsv")).expect("read tsv");
        let first_data = tsv.lines().nth(1).unwrap();
        assert!(first_data.contains('\t'), "TSV should use tab delimiter");
    });
}

#[test]
fn test_parquet_output_exists() {
    with_temp_dir(|dir| {
        let input = fixture_path("small_low_uniq.fastq");
        let (_, _, ok) =
            run_seqtable(&[input.to_str().unwrap(), "-o", dir.to_str().unwrap(), "-q"]);
        assert!(ok);
        assert!(dir.join("small_low_uniq.parquet").exists());
    });
}

#[test]
fn test_reject_fasta() {
    with_temp_dir(|dir| {
        // Create a fake fasta file
        let fasta = dir.join("test.fasta");
        std::fs::write(&fasta, ">seq1\nACGT\n").unwrap();

        let (_, stderr, ok) =
            run_seqtable(&[fasta.to_str().unwrap(), "-o", dir.to_str().unwrap(), "-q"]);
        assert!(!ok, "should reject FASTA");
        assert!(
            stderr.contains("Unsupported file format"),
            "error should mention unsupported format: {stderr}"
        );
    });
}

#[test]
fn test_multiple_files() {
    with_temp_dir(|dir| {
        let input1 = fixture_path("small_low_uniq.fastq");
        let input2 = fixture_path("small_high_uniq.fastq");
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

        // Both output files should exist
        assert!(dir.join("small_low_uniq.csv").exists());
        assert!(dir.join("small_high_uniq.csv").exists());
    });
}

#[test]
fn test_amplicon_variable_length() {
    with_temp_dir(|dir| {
        let input = fixture_path("small_amplicon.fastq");
        let (_, _, ok) = run_seqtable(&[
            input.to_str().unwrap(),
            "-o",
            dir.to_str().unwrap(),
            "-f",
            "csv",
            "-q",
        ]);
        assert!(ok);

        let csv = std::fs::read_to_string(dir.join("small_amplicon.csv")).expect("read csv");
        let counts = parse_csv_counts(&csv);

        assert_eq!(counts.len(), 25);
        let total: u64 = counts.values().sum();
        assert_eq!(total, 100);

        // Verify sequences have different lengths (amplicon variable length handling)
        let lengths: std::collections::HashSet<usize> = counts.keys().map(|s| s.len()).collect();
        assert!(
            lengths.len() > 1,
            "amplicon fixture should produce sequences of different lengths"
        );
    });
}

#[test]
fn test_status_messages_on_stderr() {
    with_temp_dir(|dir| {
        let input = fixture_path("small_low_uniq.fastq");
        let (stdout, stderr, ok) = run_seqtable(&[
            input.to_str().unwrap(),
            "-o",
            dir.to_str().unwrap(),
            "-f",
            "csv",
        ]);
        assert!(ok);

        // stdout should be empty (data goes to file)
        assert!(stdout.is_empty(), "stdout should be empty, got: {stdout}");
        // stderr should have status messages
        assert!(
            stderr.contains("seqtable"),
            "stderr should have status: {stderr}"
        );
    });
}
