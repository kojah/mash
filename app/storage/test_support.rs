//! Canonical SQLite access for fixtures and diagnostics.
use super::*;

/// Inspect retained silver projections for ingestion assertions. This is a
/// case-insensitive substring check, with no retrieval, ranking or gold access.
pub(crate) fn projections_with_text(
    storage: &Storage,
    text: &str,
    provider: Option<crate::provider::Provider>,
) -> Vec<Message> {
    let mut statement = storage
        .connection
        .prepare(
            "SELECT m.id FROM objects m JOIN objects c ON c.id=m.conversation_id
         AND c.origin='provider' AND c.object_role='container'
         WHERE m.origin='provider' AND m.object_role='entry'
         AND (?2 IS NULL OR c.source=?2)
         AND instr(lower(COALESCE(m.search_projection,m.content)),lower(?1))>0
         ORDER BY m.id",
        )
        .unwrap();
    let ids = statement
        .query_map(rusqlite::params![text, provider], |row| {
            row.get::<_, String>(0)
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    ids.into_iter()
        .map(|id| {
            storage
                .view_content(&id, 0, MessageContent::SearchProjection)
                .unwrap()
                .remove(0)
        })
        .collect()
}

pub(crate) fn connect(path: &Path) -> Connection {
    let connection = Connection::open(path).expect("open local silver");
    crate::storage::packed::register(&connection).expect("register silver codec");
    connection
}

/// Apply raw canonical SQL with the node publication journal installed.
pub(crate) fn write(path: &Path, sql: &str) {
    Storage::open_existing(path)
        .expect("local index")
        .connection
        .execute_batch(sql)
        .expect("raw write");
}
