//! Tells open dashboards that history changed, as Server-Sent Events.
//!
//! There are two sources, both used only while at least one browser listens:
//!
//! - This machine's index. Indexing happens in other processes (`mash index`,
//!   its watcher, MCP), so the dashboard learns of it from silver itself:
//!   every committed change to a chat or message moves
//!   `silver_state.revision`. One task reads that revision through a
//!   read-only connection every few seconds and, when it has moved, names the
//!   chats whose content differs.
//! - The shared index, when one is configured. The shared server announces
//!   each node's publication on its own event stream; the dashboard listens
//!   with this node's token, which never reaches the browser, and passes the
//!   announcements on.
//!
//! An event carries node and chat IDs and a revision, never message text; the
//! browser rereads what it shows through the ordinary history routes.
//!
//! Everything is bounded: a fixed number of streams, a small broadcast buffer
//! (a stream that falls behind is told to reread everything), a capped list
//! of chat IDs per event, and one connection to the shared server.
use super::Dash;
use crate::AppError;
use crate::config::SharedConfig;
use crate::shared::protocol::NodeChange;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{Semaphore, broadcast, watch};

/// Browser tabs that may listen at once; each holds one connection open.
const MAX_STREAMS: usize = 16;
/// Chats one event names before it says "everything" instead.
const MAX_CHATS: usize = 100;
/// Events a slow stream may leave unread before it is told to reread everything.
const BUFFER: usize = 16;

/// What changed in one machine's history since the previous event.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub(super) struct Change {
    /// The machine whose published history changed; none for this machine's own index.
    node: Option<String>,
    /// That history's silver revision after the change.
    revision: i64,
    /// Chats that were added, removed or changed. Empty when `all` is set.
    chats: Vec<String>,
    /// Too much changed to name, or changes may have been missed: reread everything shown.
    all: bool,
    /// Whether that machine's memories may have changed.
    records: bool,
}

/// The shared index this dashboard also reads.
#[derive(Clone)]
pub(super) struct Upstream {
    pub(super) config: SharedConfig,
    /// Whether this node's own published chats are shown from the shared
    /// index, as on a dashboard without a local one.
    pub(super) own: bool,
}

/// How often silver is read and how often an idle stream is kept alive.
#[derive(Clone, Copy)]
pub(super) struct Pace {
    pub(super) poll: Duration,
    pub(super) heartbeat: Duration,
}

impl Default for Pace {
    fn default() -> Self {
        Self {
            poll: Duration::from_secs(2),
            // Well inside the idle timeout of any proxy in front of the dashboard.
            heartbeat: Duration::from_secs(15),
        }
    }
}

pub(super) struct Live {
    /// The local index; none when this dashboard reads only the shared one.
    database: Option<PathBuf>,
    shared: Option<Upstream>,
    changes: broadcast::Sender<Change>,
    streams: Arc<Semaphore>,
    /// Whether the task that watches both sources is running. Held while a stream
    /// subscribes and while the task decides to stop, so neither misses the other.
    watching: Arc<Mutex<bool>>,
    closed: watch::Sender<bool>,
    pace: Pace,
}

impl Live {
    pub(super) fn new(database: Option<PathBuf>, shared: Option<Upstream>) -> Self {
        Self::paced(database, shared, Pace::default(), MAX_STREAMS)
    }

    pub(super) fn paced(
        database: Option<PathBuf>,
        shared: Option<Upstream>,
        pace: Pace,
        streams: usize,
    ) -> Self {
        Self {
            database,
            shared,
            changes: broadcast::Sender::new(BUFFER),
            streams: Arc::new(Semaphore::new(streams)),
            watching: Arc::new(Mutex::new(false)),
            closed: watch::Sender::new(false),
            pace,
        }
    }

    /// Ends every stream, so a stopping server is not kept open by them.
    pub(super) fn close(&self) {
        self.closed.send_replace(true);
    }

    fn subscribe(&self) -> broadcast::Receiver<Change> {
        let mut watching = self.watching.lock().unwrap_or_else(PoisonError::into_inner);
        let receiver = self.changes.subscribe();
        if !*watching && (self.database.is_some() || self.shared.is_some()) {
            *watching = true;
            tokio::spawn(watch(
                self.database.clone(),
                self.shared.clone(),
                self.changes.clone(),
                self.watching.clone(),
                self.closed.subscribe(),
                self.pace.poll,
            ));
        }
        receiver
    }
}

/// One revision of silver: what each chat's content hashed to.
struct Snapshot {
    revision: i64,
    /// What the memories add up to; they change without moving the revision.
    records: i64,
    chats: HashMap<String, u64>,
}

/// Reads silver unless it is still at `known`. A missing index is an empty one.
fn snapshot(path: &Path, known: Option<(i64, i64)>) -> rusqlite::Result<Option<Snapshot>> {
    if !path.is_file() {
        return Ok((known != Some((-1, 0))).then(|| Snapshot {
            revision: -1,
            records: 0,
            chats: HashMap::new(),
        }));
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.busy_timeout(Duration::from_secs(1))?;
    // The revision and the chats must describe the same committed state.
    connection.execute_batch("BEGIN")?;
    let revision: i64 = connection.query_row(
        "SELECT revision FROM silver_state WHERE singleton=1",
        [],
        |row| row.get(0),
    )?;
    // Every change to a memory adds a revision or marks it forgotten.
    let records: i64 = connection.query_row(
        "SELECT COUNT(*)+COALESCE(SUM(revision),0)+COALESCE(SUM(forgotten),0) FROM records",
        [],
        |row| row.get(0),
    )?;
    if known == Some((revision, records)) {
        return Ok(None);
    }
    // The fingerprint covers a chat's metadata and messages; the last ordinal
    // also catches messages committed apart from their chat row.
    let mut statement = connection.prepare(
        "SELECT c.id,c.source_fingerprint,(SELECT MAX(m.ordinal) FROM messages m WHERE m.chat_id=c.id) FROM chats c",
    )?;
    let chats = statement
        .query_map([], |row| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            row.get::<_, String>(1)?.hash(&mut hasher);
            row.get::<_, Option<i64>>(2)?.hash(&mut hasher);
            Ok((row.get::<_, String>(0)?, hasher.finish()))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some(Snapshot {
        revision,
        records,
        chats,
    }))
}

/// The chats that differ between two snapshots; none when nothing a reader sees did.
fn difference(before: &Snapshot, after: &Snapshot) -> Option<Change> {
    let mut chats: Vec<String> = after
        .chats
        .iter()
        .filter(|(id, hash)| before.chats.get(*id) != Some(hash))
        .map(|(id, _)| id)
        .chain(
            before
                .chats
                .keys()
                .filter(|id| !after.chats.contains_key(*id)),
        )
        .cloned()
        .collect();
    let records = before.records != after.records;
    if chats.is_empty() && !records {
        return None;
    }
    let all = chats.len() > MAX_CHATS;
    if all {
        chats.clear();
    }
    chats.sort_unstable();
    Some(Change {
        node: None,
        revision: after.revision,
        chats,
        all,
        records,
    })
}

/// Watches both sources until no stream listens or the server stops.
async fn watch(
    database: Option<PathBuf>,
    shared: Option<Upstream>,
    changes: broadcast::Sender<Change>,
    watching: Arc<Mutex<bool>>,
    mut closed: watch::Receiver<bool>,
    interval: Duration,
) {
    let sources = async {
        tokio::join!(
            async {
                match database {
                    Some(database) => watch_silver(database, &changes, interval).await,
                    None => std::future::pending().await,
                }
            },
            async {
                match shared {
                    Some(shared) => relay_shared(shared, &changes).await,
                    None => std::future::pending().await,
                }
            },
        );
    };
    let unwanted = async {
        loop {
            tokio::select! {
                _ = closed.wait_for(|closed| *closed) => {}
                () = tokio::time::sleep(interval) => {}
            }
            let mut watching = watching.lock().unwrap_or_else(PoisonError::into_inner);
            if *closed.borrow() || changes.receiver_count() == 0 {
                *watching = false;
                return;
            }
        }
    };
    tokio::select! {
        () = sources => {}
        () = unwanted => {}
    }
}

/// Reads silver at each interval and announces what changed in between.
async fn watch_silver(database: PathBuf, changes: &broadcast::Sender<Change>, interval: Duration) {
    let mut known: Option<Snapshot> = None;
    loop {
        let path = database.clone();
        let revision = known
            .as_ref()
            .map(|snapshot| (snapshot.revision, snapshot.records));
        // A read that fails (a busy or half-built index) is tried again next time.
        if let Ok(Ok(Some(next))) =
            tokio::task::spawn_blocking(move || snapshot(&path, revision)).await
        {
            // The first reading is what browsers have just loaded, not a change.
            if let Some(change) = known.as_ref().and_then(|known| difference(known, &next)) {
                let _ = changes.send(change);
            }
            known = Some(next);
        }
        tokio::time::sleep(interval).await;
    }
}

/// The longest one line of the shared server's stream may be: a hundred chat IDs.
const MAX_LINE: usize = 256 * 1024;

/// How long to wait before listening to the shared server again: 1s, doubling, at most 60s.
fn retry_delay(failures: u32) -> Duration {
    Duration::from_secs(1 << failures.saturating_sub(1).min(6)).min(Duration::from_secs(60))
}

/// Everything shown may be out of date.
fn everything() -> Change {
    Change {
        node: None,
        revision: 0,
        chats: Vec::new(),
        all: true,
        records: true,
    }
}

/// Listens to the shared server and passes on what other nodes published.
/// A lost connection is reopened after a growing wait, and once it is back
/// browsers reread everything, since changes in between went unheard.
async fn relay_shared(shared: Upstream, changes: &broadcast::Sender<Change>) {
    use futures_util::StreamExt;
    let mut heard = false;
    let mut failures = 0;
    loop {
        if let Ok(response) = crate::shared::events(&shared.config).await {
            failures = 0;
            if heard {
                let _ = changes.send(everything());
            }
            heard = true;
            let mut body = response.bytes_stream();
            let mut pending = Vec::new();
            while let Some(Ok(bytes)) = body.next().await {
                pending.extend_from_slice(&bytes);
                while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                    let line: Vec<u8> = pending.drain(..=end).collect();
                    if let Some(change) = shared_change(&line, &shared) {
                        let _ = changes.send(change);
                    }
                }
                if pending.len() > MAX_LINE {
                    break;
                }
            }
        }
        failures += 1;
        tokio::time::sleep(retry_delay(failures)).await;
    }
}

/// One line of the shared server's stream as a change to pass on, when it is one.
fn shared_change(line: &[u8], shared: &Upstream) -> Option<Change> {
    let data = std::str::from_utf8(line).ok()?.strip_prefix("data:")?;
    let change: NodeChange = serde_json::from_str(data.trim()).ok()?;
    // This machine's own chats are shown from its index, which is watched directly.
    if change.node == shared.config.node_id && !shared.own {
        return None;
    }
    Some(Change {
        node: Some(change.node),
        revision: change.revision,
        chats: change.chats,
        all: change.all,
        records: change.records,
    })
}

/// The next event for one stream, or none when the stream should end.
async fn next(
    receiver: &mut broadcast::Receiver<Change>,
    closed: &mut watch::Receiver<bool>,
) -> Option<Change> {
    let mut behind = false;
    loop {
        tokio::select! {
            _ = closed.wait_for(|closed| *closed) => return None,
            change = receiver.recv() => match change {
                Ok(change) if behind => {
                    return Some(Change { node: None, chats: Vec::new(), all: true, ..change });
                }
                Ok(change) => return Some(change),
                Err(RecvError::Lagged(_)) => behind = true,
                Err(RecvError::Closed) => return None,
            },
        }
    }
}

pub(super) fn routes(router: aide::axum::ApiRouter<Arc<Dash>>) -> aide::axum::ApiRouter<Arc<Dash>> {
    router.api_route(
        "/api/events",
        aide::axum::routing::get_with(
            events,
            super::contract::reads::<Change>(
                "/api/events",
                "A text/event-stream of `change` events, each naming the chats that changed.",
            ),
        ),
    )
}

/// A stream of `change` events, with comment lines in between so that proxies
/// keep an idle connection open.
async fn events(State(state): State<Arc<Dash>>) -> Response {
    let live = &state.live;
    let Ok(permit) = live.streams.clone().try_acquire_owned() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::RETRY_AFTER, "30")],
            axum::Json(AppError::configuration(
                "too many dashboard tabs are listening for updates; close one",
            )),
        )
            .into_response();
    };
    let stream = futures_util::stream::unfold(
        (live.subscribe(), live.closed.subscribe(), permit),
        |(mut receiver, mut closed, permit)| async move {
            let change = next(&mut receiver, &mut closed).await?;
            let event = Event::default().event("change").json_data(&change).ok()?;
            Some((
                Ok::<_, std::convert::Infallible>(event),
                (receiver, closed, permit),
            ))
        },
    );
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(live.pace.heartbeat))
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::{Change, Live, MAX_CHATS, Pace, Snapshot, difference};
    use futures_util::StreamExt;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn snapshot(revision: i64, chats: &[(&str, u64)]) -> Snapshot {
        Snapshot {
            revision,
            records: 0,
            chats: chats
                .iter()
                .map(|(id, hash)| ((*id).to_owned(), *hash))
                .collect::<HashMap<_, _>>(),
        }
    }

    #[test]
    fn a_change_names_added_removed_and_altered_chats_and_nothing_else() {
        let before = snapshot(1, &[("kept", 1), ("altered", 1), ("removed", 1)]);
        let after = snapshot(4, &[("kept", 1), ("altered", 2), ("added", 1)]);
        assert_eq!(
            difference(&before, &after),
            Some(Change {
                node: None,
                revision: 4,
                chats: vec!["added".into(), "altered".into(), "removed".into()],
                all: false,
                records: false,
            })
        );
        // A memory saved or revised is news with no chat to name.
        let saved = Snapshot {
            records: 3,
            ..snapshot(1, &[("kept", 1), ("altered", 1), ("removed", 1)])
        };
        assert_eq!(
            difference(&before, &saved),
            Some(Change {
                node: None,
                revision: 1,
                chats: Vec::new(),
                all: false,
                records: true,
            })
        );
        // Turns are re-derived after every write; that alone shows a reader nothing new.
        assert_eq!(
            difference(
                &before,
                &snapshot(9, &[("kept", 1), ("altered", 1), ("removed", 1)])
            ),
            None
        );
    }

    #[test]
    fn too_many_changed_chats_are_not_named() {
        let ids: Vec<String> = (0..=MAX_CHATS)
            .map(|index| format!("chat-{index}"))
            .collect();
        let chats: Vec<(&str, u64)> = ids.iter().map(|id| (id.as_str(), 1)).collect();
        let change = difference(&snapshot(1, &[]), &snapshot(2, &chats)).unwrap();
        assert!(change.all);
        assert_eq!(change.chats, Vec::<String>::new());
    }

    fn index(directory: &Path) -> PathBuf {
        let database = directory.join("index.sqlite3");
        let mut storage = crate::storage::Storage::open_writer(&database).unwrap();
        storage.commit_writer().unwrap();
        database
    }

    fn write(database: &Path, sql: &str) {
        let connection = rusqlite::Connection::open(database).unwrap();
        crate::storage::packed::register(&connection).unwrap();
        connection.execute_batch(sql).unwrap();
    }

    async fn serve(
        live: Live,
        token: Option<&str>,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = super::super::router(std::sync::Arc::new(super::Dash {
            console: crate::mcp::console::Console::new(crate::core::Paths::default()),
            paths: crate::core::Paths::default(),
            hosts: super::super::allowed_hosts(address),
            loopback: token.is_none(),
            access: super::super::access::Access::new(token),
            shared_only: false,
            model_work: tokio::sync::Semaphore::new(0),
            services: super::super::services::Services::load(None).unwrap(),
            live,
            questions: std::sync::Arc::new(tokio::sync::Semaphore::new(2)),
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (address, server)
    }

    /// Reads the stream until `wanted` appears in it.
    async fn read_until(response: reqwest::Response, wanted: &str) -> String {
        let mut body = response.bytes_stream();
        let mut text = String::new();
        tokio::time::timeout(Duration::from_secs(20), async {
            while !text.contains(wanted) {
                let chunk = body.next().await.expect("the stream ended").unwrap();
                text.push_str(std::str::from_utf8(&chunk).unwrap());
            }
        })
        .await
        .unwrap_or_else(|_| panic!("no {wanted:?} in {text:?}"));
        text
    }

    #[tokio::test]
    async fn a_silver_change_reaches_a_listening_browser_as_chat_ids_without_text() {
        let directory = tempfile::tempdir().unwrap();
        let database = index(directory.path());
        write(
            &database,
            "INSERT INTO chats(id,provider,source_fingerprint) VALUES('quiet','codex','a'),('busy','codex','a'); INSERT INTO messages(id,chat_id,ordinal,role,content_packed) VALUES('first','busy',0,'user','secret words');",
        );
        let pace = Pace {
            poll: Duration::from_millis(50),
            heartbeat: Duration::from_millis(100),
        };
        let (address, server) =
            serve(Live::paced(Some(database.clone()), None, pace, 4), None).await;
        let response = reqwest::get(format!("http://{address}/api/events"))
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert!(response.headers().get("content-encoding").is_none());
        // An idle stream carries comments, and what was there at the start is not news.
        let mut body = response.bytes_stream();
        let mut idle = String::new();
        while idle.matches(":\n\n").count() < 2 {
            let chunk = tokio::time::timeout(Duration::from_secs(20), body.next())
                .await
                .unwrap();
            idle.push_str(std::str::from_utf8(&chunk.unwrap().unwrap()).unwrap());
        }
        assert!(!idle.contains("event:"), "{idle:?}");
        // A message committed apart from its chat row still counts.
        write(
            &database,
            "INSERT INTO messages(id,chat_id,ordinal,role,content_packed) VALUES('second','busy',1,'assistant','more secret words');",
        );
        let response = reqwest::get(format!("http://{address}/api/events"))
            .await
            .unwrap();
        // The stream opened before the write and one opened after it hear the same thing.
        write(
            &database,
            "INSERT INTO chats(id,provider,source_fingerprint) VALUES('new','codex','a');",
        );
        let text = read_until(response, "\n\n").await;
        let data = text
            .lines()
            .find_map(|line| line.strip_prefix("data:"))
            .unwrap_or_else(|| panic!("no data in {text:?}"));
        assert!(
            text.contains("event: change") || text.contains("event:change"),
            "{text:?}"
        );
        let change: serde_json::Value = serde_json::from_str(data.trim()).unwrap();
        assert_eq!(change["all"], false);
        let chats = change["chats"].as_array().unwrap();
        assert!(
            chats.iter().all(|id| id == "busy" || id == "new"),
            "{change}"
        );
        assert_ne!(chats.len(), 0);
        assert!(change["revision"].as_i64().unwrap() > 0);
        assert!(!text.contains("secret"));
        server.abort();
    }

    #[tokio::test]
    async fn a_reachable_dashboard_streams_only_after_sign_in_and_to_its_own_host() {
        let (address, server) = serve(Live::new(None, None), Some(TOKEN)).await;
        let client = reqwest::Client::new();
        let url = format!("http://{address}/api/events");
        let stranger = client.get(&url).send().await.unwrap();
        assert_eq!(stranger.status(), reqwest::StatusCode::UNAUTHORIZED);
        let elsewhere = client
            .get(&url)
            .bearer_auth(TOKEN)
            .header("Host", "attacker.invalid")
            .send()
            .await
            .unwrap();
        assert_eq!(elsewhere.status(), reqwest::StatusCode::MISDIRECTED_REQUEST);
        let post = client.post(&url).bearer_auth(TOKEN).send().await.unwrap();
        assert_eq!(post.status(), reqwest::StatusCode::METHOD_NOT_ALLOWED);
        let signed_in = client.get(&url).bearer_auth(TOKEN).send().await.unwrap();
        assert_eq!(signed_in.status(), reqwest::StatusCode::OK);
        assert_eq!(signed_in.headers()["content-type"], "text/event-stream");
        server.abort();
    }

    #[tokio::test]
    async fn streams_are_capped_and_a_closed_one_frees_its_place() {
        let pace = Pace {
            poll: Duration::from_millis(50),
            heartbeat: Duration::from_millis(50),
        };
        let live = Live::paced(None, None, pace, 2);
        let (address, server) = serve(live, None).await;
        let url = format!("http://{address}/api/events");
        let first = reqwest::get(&url).await.unwrap();
        let second = reqwest::get(&url).await.unwrap();
        assert_eq!(second.status(), reqwest::StatusCode::OK);
        let third = reqwest::get(&url).await.unwrap();
        assert_eq!(third.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(third.headers()["retry-after"], "30");
        drop(first);
        // The server notices the closed connection when it next writes a heartbeat.
        let mut status = reqwest::StatusCode::SERVICE_UNAVAILABLE;
        for _ in 0..100 {
            status = reqwest::get(&url).await.unwrap().status();
            if status == reqwest::StatusCode::OK {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(status, reqwest::StatusCode::OK);
        drop(second);
        server.abort();
    }

    #[tokio::test]
    async fn what_other_machines_publish_is_passed_on_and_a_lost_upstream_means_reread() {
        // A shared server that says two things and hangs up, every time.
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_url = format!("http://{}", upstream.local_addr().unwrap());
        let said = axum::Router::new().route(
            "/v1/events",
            axum::routing::get(|headers: axum::http::HeaderMap| async move {
                assert!(headers.contains_key("authorization"));
                (
                    [("content-type", "text/event-stream")],
                    concat!(
                        ": hello\n\n",
                        "event: change\ndata: {\"node\":\"here\",\"revision\":3,\"chats\":[\"mine\"],\"all\":false}\n\n",
                        "event: change\ndata: {\"node\":\"there\",\"revision\":7,\"chats\":[\"theirs\"],\"all\":false,\"records\":true}\n\n",
                        "data: <script>\n\n",
                    ),
                )
            }),
        );
        let shared = tokio::spawn(async move { axum::serve(upstream, said).await.unwrap() });
        let config = crate::config::SharedConfig {
            group_id: "group".into(),
            node_id: "here".into(),
            api_url,
            // Any variable that is always set; the stand-in server takes any token.
            token_env: "PATH".into(),
            publication_redaction: crate::redaction::Policy::ON,
        };
        let live = Live::new(None, Some(super::Upstream { config, own: false }));
        let (address, server) = serve(live, None).await;
        let response = reqwest::get(format!("http://{address}/api/events"))
            .await
            .unwrap();
        // The second connection to the shared server is a reconnection.
        let text = read_until(response, "\"all\":true").await;
        let changes: Vec<serde_json::Value> = text
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(|data| serde_json::from_str(data.trim()).unwrap())
            .collect();
        assert_eq!(
            changes[0],
            serde_json::json!({"node": "there", "revision": 7, "chats": ["theirs"], "all": false, "records": true})
        );
        assert_eq!(changes[1]["all"], true);
        // This machine's own chats are shown from its index, not the shared one.
        assert!(!text.contains("mine"));
        server.abort();
        shared.abort();
    }

    #[test]
    fn a_dashboard_without_its_own_index_hears_its_own_node_too() {
        let config = crate::config::SharedConfig {
            group_id: "group".into(),
            node_id: "here".into(),
            api_url: String::new(),
            token_env: String::new(),
            publication_redaction: crate::redaction::Policy::ON,
        };
        let line = br#"data: {"node":"here","revision":3,"chats":["mine"],"all":false}"#;
        let own = super::Upstream {
            config: config.clone(),
            own: true,
        };
        assert_eq!(
            super::shared_change(line, &own).unwrap().node.as_deref(),
            Some("here")
        );
        assert_eq!(
            super::shared_change(line, &super::Upstream { config, own: false }),
            None
        );
        assert_eq!(super::shared_change(b"event: change", &own), None);
    }

    #[test]
    fn the_shared_server_is_retried_after_1s_doubling_to_a_minute() {
        let waits: Vec<u64> = [1, 2, 3, 7, 8, 50]
            .into_iter()
            .map(|failures| super::retry_delay(failures).as_secs())
            .collect();
        assert_eq!(waits, [1, 2, 4, 60, 60, 60]);
    }

    #[tokio::test]
    async fn closing_ends_open_streams() {
        let live = Live::new(None, None);
        let mut receiver = live.subscribe();
        let mut closed = live.closed.subscribe();
        live.close();
        assert_eq!(super::next(&mut receiver, &mut closed).await, None);
    }

    #[tokio::test]
    async fn a_stream_that_fell_behind_is_told_to_reread_everything() {
        let live = Live::new(None, None);
        let mut receiver = live.subscribe();
        let mut closed = live.closed.subscribe();
        for revision in 0..=i64::try_from(super::BUFFER).unwrap() {
            let _ = live.changes.send(Change {
                node: None,
                revision,
                chats: vec!["one".into()],
                all: false,
                records: false,
            });
        }
        let change = super::next(&mut receiver, &mut closed).await.unwrap();
        assert!(change.all);
        assert_eq!(change.chats, Vec::<String>::new());
    }
}
