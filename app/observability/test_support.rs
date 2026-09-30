use std::io::Write;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(super) struct Capture(pub(super) Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) fn capture() -> (Arc<Mutex<Vec<u8>>>, tracing::Dispatch) {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let writer = Capture(bytes.clone());
    let dispatcher = tracing::Dispatch::new(super::subscriber(move || writer.clone()));
    (bytes, dispatcher)
}
