//! Opt-in isolated HTTP wire-buffer control; fixture server runs separately.
use super::*;

struct Digest(blake3::Hasher);
impl io::Write for Digest {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
#[ignore = "requires an externally served frozen large NodeState fixture"]
async fn large_state_wire_memory_control() {
    static CAPACITY: Admission = Admission::new(1, 1);
    let url = std::env::var("MASH_TEST_STATE_URL").unwrap();
    let mode = std::env::var("MASH_TEST_STATE_MODE").unwrap();
    let expected: usize = std::env::var("MASH_TEST_STATE_RECORDS")
        .unwrap()
        .parse()
        .unwrap();
    let started = std::time::Instant::now();
    let response = reqwest::Client::new().get(url).send().await.unwrap();
    let state: NodeState = match mode.as_str() {
        "buffered" => {
            // Previous implementation: retain the complete wire alongside the
            // growing typed model, without an extra Bytes-to-Vec copy.
            let bytes = response.bytes().await.unwrap();
            crate::blocking(move || Ok(serde_json::from_slice::<NodeState>(&bytes).unwrap()))
                .await
                .unwrap()
        }
        "adaptive" => read(
            response,
            Endpoint::Silver,
            CAPACITY.bulk.acquire().await.unwrap(),
        )
        .await
        .unwrap(),
        _ => panic!("unknown control mode"),
    };
    let elapsed = started.elapsed();
    assert_eq!(state.tombstones.len(), expected);
    let mut digest = Digest(blake3::Hasher::new());
    serde_json::to_writer(&mut digest, &state).unwrap();
    println!(
        "mode={mode} records={expected} read_decode_ms={} typed_blake3={}",
        elapsed.as_millis(),
        digest.0.finalize()
    );
}
