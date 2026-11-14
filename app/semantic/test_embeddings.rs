//! Deterministic inference for storage tests; never compiled into Mash binaries.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

static FIXTURES: LazyLock<Mutex<HashMap<PathBuf, Arc<State>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct State {
    dimensions: usize,
    pub calls: Mutex<Vec<usize>>,
}

impl State {
    pub fn embed(&self, count: usize) -> Vec<Vec<f32>> {
        self.calls.lock().unwrap().push(count);
        let mut vector = vec![0.0; self.dimensions];
        vector[0] = 1.0;
        vec![vector; count]
    }
}

pub struct Fixture {
    root: PathBuf,
    pub state: Arc<State>,
    directory: Option<tempfile::TempDir>,
}

impl Fixture {
    pub fn new(dimensions: usize) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut fixture = Self::at(directory.path(), dimensions);
        fixture.directory = Some(directory);
        fixture
    }

    pub fn at(root: &Path, dimensions: usize) -> Self {
        assert!(dimensions > 0);
        let state = Arc::new(State {
            dimensions,
            calls: Mutex::new(Vec::new()),
        });
        assert!(
            FIXTURES
                .lock()
                .unwrap()
                .insert(root.to_path_buf(), Arc::clone(&state))
                .is_none()
        );
        Self {
            root: root.to_path_buf(),
            state,
            directory: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        FIXTURES.lock().unwrap().remove(&self.root);
        drop(self.directory.take());
    }
}

pub(super) fn load(root: &Path) -> Option<super::LoadedIndexer> {
    let state = FIXTURES.lock().unwrap().get(root).cloned()?;
    Some(super::models::indexer::LoadedIndexer::fixture(state))
}
