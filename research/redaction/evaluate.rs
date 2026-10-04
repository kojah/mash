//! Synthetic-only comparison; see README.md for reproduction.
use rusty_hog_scanner::SecretScannerBuilder;
use secrets_scanner::{ScanConfig, Scanner};
use serde::Deserialize;
use std::{hint::black_box, time::Instant};
#[path = "../../app/redaction/detection.rs"]
mod detection;

#[derive(Deserialize)]
struct Fixture {
    name: String,
    text: String,
    secrets: Vec<String>,
}

fn main() {
    let fixtures: Vec<Fixture> =
        serde_json::from_str(&std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap())
            .unwrap();
    let start = Instant::now();
    black_box(detection::text("Hello world"));
    println!(
        "mash_policy initialization_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.0
    );
    let start = Instant::now();
    let scanner = Scanner::from_bundled()
        .unwrap()
        .with_config(ScanConfig::proxy());
    println!(
        "secrets_scanner initialization_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.0
    );
    let start = Instant::now();
    let hog = SecretScannerBuilder::new().build();
    println!(
        "rusty_hog initialization_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.0
    );
    for (name, scan, spans) in [
        (
            "secrets_scanner",
            Box::new(|text: &str| {
                let output = scanner.scan_proxy(text.as_bytes()).unwrap();
                String::from_utf8(output.redacted).unwrap()
            }) as Box<dyn Fn(&str) -> String>,
            Box::new(|text: &str| {
                scanner
                    .scan_proxy(text.as_bytes())
                    .unwrap()
                    .findings
                    .into_iter()
                    .map(|f| f.secret_start_offset..f.secret_end_offset)
                    .collect()
            }) as Box<dyn Fn(&str) -> Vec<std::ops::Range<usize>>>,
        ),
        (
            "rusty_hog",
            Box::new(|text: &str| {
                let mut ranges = hog
                    .matches_entropy(text.as_bytes())
                    .into_values()
                    .flatten()
                    .map(|m| m.range())
                    .collect::<Vec<_>>();
                ranges.sort_by_key(|r| (r.start, r.end));
                let mut merged: Vec<std::ops::Range<usize>> = Vec::new();
                for range in ranges {
                    if let Some(previous) = merged.last_mut()
                        && range.start <= previous.end
                    {
                        previous.end = previous.end.max(range.end);
                    } else {
                        merged.push(range);
                    }
                }
                let mut output = String::new();
                let mut offset = 0;
                for r in merged {
                    output.push_str(&text[offset..r.start]);
                    output.push_str("[REDACTED_SECRET]");
                    offset = r.end;
                }
                output.push_str(&text[offset..]);
                output
            }),
            Box::new(|text: &str| {
                hog.matches_entropy(text.as_bytes())
                    .into_values()
                    .flatten()
                    .map(|m| m.range())
                    .collect()
            }),
        ),
        (
            "mash_policy",
            Box::new(detection::text),
            Box::new(detection::ranges),
        ),
    ] {
        let mut missed = 0;
        let mut expected = 0;
        let mut false_positives = 0;
        for f in &fixtures {
            let output = scan(&f.text);
            let covered = spans(&f.text);
            let misses = f
                .secrets
                .iter()
                .filter(|secret| {
                    assert!(
                        f.text.contains(secret.as_str()),
                        "invalid fixture {}",
                        f.name
                    );
                    f.text.match_indices(secret.as_str()).any(|(start, value)| {
                        (start..start + value.len())
                            .any(|byte| !covered.iter().any(|range| range.contains(&byte)))
                    })
                })
                .count();
            expected += f.secrets.len();
            missed += misses;
            let fp = f.secrets.is_empty() && output != f.text;
            false_positives += usize::from(fp);
            println!("{name} case={} misses={misses} benign_changed={fp}", f.name);
        }
        let start = Instant::now();
        let mut bytes = 0;
        for _ in 0..100 {
            for f in &fixtures {
                black_box(scan(black_box(&f.text)));
                bytes += f.text.len();
            }
        }
        println!(
            "{name} summary caught={}/{expected} benign_changed={false_positives} throughput_mib_s={:.2}",
            expected - missed,
            bytes as f64 / (1024.0 * 1024.0) / start.elapsed().as_secs_f64()
        );
    }
}
