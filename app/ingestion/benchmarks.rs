//! Explicit synthetic aggregate parser-retention diagnostic, without inference.
use super::*;

#[test]
#[ignore = "manual RSS diagnostic requires explicit synthetic root and expected message count"]
fn benchmark_parser_admission_retention() {
    let root = PathBuf::from(std::env::var_os("MASH_BENCH_PARSER_ROOT").expect("synthetic root"));
    let expected: u64 = std::env::var("MASH_BENCH_PARSER_MESSAGES")
        .expect("expected canonical message count")
        .parse()
        .unwrap();
    let policy = std::env::var("MASH_BENCH_PARSER_POLICY").expect("parallel or bounded");
    let admission = match policy.as_str() {
        "parallel" => admission::Admission::with_limit(u64::MAX),
        "bounded" => admission::Admission::new(),
        _ => panic!("invalid parser policy"),
    };
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    storage.defer_search_updates().unwrap();
    let roots = [root];
    let discovery = discover_history_files(Provider::ClaudeCode, &roots, true).unwrap();
    let mut summary = IndexSummary::default();
    let started = Instant::now();
    index_provider_pipeline(
        &mut storage,
        ProviderScan {
            provider: Provider::ClaudeCode,
            roots: &roots,
            cutoff_modified_ns: None,
            parse: Provider::ClaudeCode.parser(),
            authoritative: false,
        },
        discovery,
        &mut summary,
        &mut IndexProgress::new(),
        &admission,
    )
    .unwrap();
    storage.commit_writer().unwrap();
    let counts = storage.counts().unwrap();
    assert_eq!(counts.messages, expected);
    assert_eq!(summary.malformed_records, 0);
    assert_eq!(summary.changed_messages, expected);
    eprintln!(
        "policy={policy} messages={} sources={} bytes={} elapsed_ms={}",
        counts.messages,
        summary.scanned_files,
        summary.processed_bytes,
        started.elapsed().as_millis()
    );
}
