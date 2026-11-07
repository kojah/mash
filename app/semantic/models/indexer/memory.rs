//! Separate-process diagnostic: compare retained bulk and watcher pools with
//! identical batches. Run each ignored test alone with installed model assets.
use super::*;
use crate::semantic::Models;

fn sample(stage: &str) {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let fields = ["VmRSS:", "RssAnon:", "RssFile:", "VmSwap:"];
    let values = fields.map(|field| {
        let value = status
            .lines()
            .find_map(|line| line.strip_prefix(field))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        (field, value)
    });
    assert!(values[0].1 > 0, "missing RSS is not a zero measurement");
    println!("memory stage={stage} kib={values:?}");
}

async fn workload(watching: bool) {
    let root = PathBuf::from(std::env::var_os("MASH_TEST_MODELS_DIR").unwrap());
    sample("before_load");
    let retained = crate::blocking(move || {
        let models = Models::new(root);
        let started = Instant::now();
        let mut loaded = models.load_indexer()?.unwrap();
        let crate::semantic::embeddings::EmbeddingPoolBackend::Cpu(workers) = &loaded.backend
        else {
            panic!("CPU model pool required");
        };
        assert_eq!(workers.len(), if watching { 1 } else { 8 });
        println!("load_ms={}", started.elapsed().as_millis());
        sample("loaded");
        let texts = (0..64)
            .map(|index| {
                if (index / 8) % 4 == 0 {
                    format!(
                        "document {index} {}",
                        "search database history ".repeat(160)
                    )
                } else {
                    format!("document {index} investigate watcher memory and indexing")
                }
            })
            .collect::<Vec<_>>();
        let refs = texts.iter().map(String::as_str).collect::<Vec<_>>();
        let mut reference = None;
        for cycle in 0..5 {
            let started = Instant::now();
            let vectors = loaded.shared_vectors(&refs)?;
            let mut digest = blake3::Hasher::new();
            for vector in vectors {
                for component in vector {
                    digest.update(&component.to_le_bytes());
                }
            }
            let digest = digest.finalize();
            if let Some(reference) = reference {
                assert_eq!(digest, reference, "repeated batches must remain identical");
            }
            reference = Some(digest);
            println!(
                "cycle={cycle} inference_ms={} digest={digest}",
                started.elapsed().as_millis()
            );
            sample("after_inference");
            if watching {
                drop(loaded);
                loaded = models.load_indexer()?.unwrap();
            }
        }
        for cycle in 0..5 {
            let started = Instant::now();
            assert_eq!(loaded.shared_vectors(&refs[8..16])?.len(), 8);
            println!(
                "small_batch={cycle} inference_ms={}",
                started.elapsed().as_millis()
            );
        }
        if watching {
            drop(loaded);
            Ok(None)
        } else {
            // Reproduce the old watcher's indefinitely retained eight-worker pool.
            Ok(Some(loaded))
        }
    })
    .await
    .unwrap();
    sample("retained");
    tokio::time::sleep(Duration::from_secs(3)).await;
    if watching {
        assert!(
            current().unwrap().0.lock().unwrap().is_none(),
            "idle timer released pool"
        );
    }
    sample("idle");
    crate::blocking(move || {
        drop(retained);
        Ok(())
    })
    .await
    .unwrap();
    sample("released");
}

#[tokio::test]
#[ignore = "manual memory diagnostic requires installed MASH_TEST_MODELS_DIR; run each policy separately"]
async fn benchmark_watcher_memory_bulk_pool() {
    workload(false).await;
}

#[tokio::test]
#[ignore = "manual memory diagnostic requires installed MASH_TEST_MODELS_DIR; run each policy separately"]
async fn benchmark_watcher_memory_bounded_pool() {
    scope_with_idle_timeout(
        async {
            for epoch in 0..3 {
                println!("epoch={epoch}");
                workload(true).await;
            }
        },
        Duration::from_secs(1),
    )
    .await;
}
