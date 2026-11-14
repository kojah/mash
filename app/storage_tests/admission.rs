use crate::storage::admission::*;

#[tokio::test]
async fn local_writer_admission_survives_cancelled_blocking_awaiter() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("index.sqlite3");
    let owner = acquire(&database).unwrap();
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let task = tokio::spawn(scope(Some(owner), async move {
        crate::blocking(move || {
            entered.send(()).unwrap();
            released.recv().unwrap();
            Ok(())
        })
        .await
    }));
    started.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(
        acquire(&database).unwrap_err().error.kind,
        crate::ErrorKind::IndexBusy
    );
    release.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if acquire(&database).is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
