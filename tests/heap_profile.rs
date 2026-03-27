//! Heap profiling with dhat.
//!
//! Run:   cargo test --test heap_profile --profile profiling -- --nocapture --test-threads=1
//! View:  open dhat-heap.json at https://nnethercote.github.io/dh_view/dh_view.html
//!
//! Requires bench fixtures:
//!   cargo run --example generate_fixtures --release -- --size bench

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use seqtable::output::{save_csv, save_parquet};
use seqtable::{count_sequences, prepare_records};
use std::path::Path;

const FIXTURE_DIR: &str = "tests/fixtures";

fn fixture_path(name: &str) -> Option<std::path::PathBuf> {
    let p = Path::new(FIXTURE_DIR).join(name);
    p.exists().then_some(p)
}

/// Full pipeline profile — generates dhat-heap.json for viewer
#[test]
fn profile_full_pipeline() {
    let path = fixture_path("bn_short_mid_10000.fastq")
        .expect("run: cargo run --example generate_fixtures --release -- --size bench");

    let _prof = dhat::Profiler::new_heap();

    // Phase 1: Count
    let (counts, total) = count_sequences(&path, 0, false).unwrap();

    // Phase 2: Prepare
    let records = prepare_records(counts, total, false);

    // Phase 3: Output
    let tmp = tempfile::NamedTempFile::with_suffix(".csv").unwrap();
    save_csv(&records, tmp.path(), b',').unwrap();

    // Profiler drops here, writes dhat-heap.json
}

/// Per-phase stats via testing mode
#[test]
fn profile_per_phase() {
    let fixtures = [
        ("short_low_5%", "bn_short_low_10000.fastq"),
        ("short_mid_50%", "bn_short_mid_10000.fastq"),
        ("short_high_90%", "bn_short_high_10000.fastq"),
        ("amp_low_5%", "bn_amp_low_10000.fastq"),
        ("amp_mid_50%", "bn_amp_mid_10000.fastq"),
        ("amp_high_90%", "bn_amp_high_10000.fastq"),
    ];

    for (label, file) in fixtures {
        let Some(path) = fixture_path(file) else {
            eprintln!("skip {file} (not found)");
            continue;
        };

        eprintln!("\n=== {label} ===");

        // Phase 1: Count
        {
            let _prof = dhat::Profiler::builder().testing().build();
            let (_counts, _total) = count_sequences(&path, 0, false).unwrap();
            let s = dhat::HeapStats::get();
            eprintln!(
                "  count:   peak={:>10} bytes ({:>6} blocks), total={:>10} bytes ({:>6} allocs)",
                s.max_bytes, s.max_blocks, s.total_bytes, s.total_blocks
            );
        }

        // Run count again to get data for next phases
        let (counts, total) = {
            let _prof = dhat::Profiler::builder().testing().build();
            count_sequences(&path, 0, false).unwrap()
        };

        // Phase 2: Prepare
        {
            let _prof = dhat::Profiler::builder().testing().build();
            let _records = prepare_records(counts.clone(), total, false);
            let s = dhat::HeapStats::get();
            eprintln!(
                "  prepare: peak={:>10} bytes ({:>6} blocks), total={:>10} bytes ({:>6} allocs)",
                s.max_bytes, s.max_blocks, s.total_bytes, s.total_blocks
            );
        }

        let records = prepare_records(counts, total, false);

        // Phase 3: Output CSV
        {
            let _prof = dhat::Profiler::builder().testing().build();
            let tmp = tempfile::NamedTempFile::with_suffix(".csv").unwrap();
            save_csv(&records, tmp.path(), b',').unwrap();
            let s = dhat::HeapStats::get();
            eprintln!(
                "  csv:     peak={:>10} bytes ({:>6} blocks), total={:>10} bytes ({:>6} allocs)",
                s.max_bytes, s.max_blocks, s.total_bytes, s.total_blocks
            );
        }

        // Phase 3: Output Parquet
        {
            let _prof = dhat::Profiler::builder().testing().build();
            let tmp = tempfile::NamedTempFile::with_suffix(".parquet").unwrap();
            save_parquet(
                &records,
                tmp.path(),
                parquet::basic::Compression::ZSTD(Default::default()),
            )
            .unwrap();
            let s = dhat::HeapStats::get();
            eprintln!(
                "  parquet: peak={:>10} bytes ({:>6} blocks), total={:>10} bytes ({:>6} allocs)",
                s.max_bytes, s.max_blocks, s.total_bytes, s.total_blocks
            );
        }
    }
}
