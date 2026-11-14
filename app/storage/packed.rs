//! Large silver text stored compressed. A packed value is a zstd frame stored
//! as a BLOB; every other value stays TEXT, so readers tell them apart by type.
//! SQL reads through `silver_text(value)` and writes through `silver_pack(text)`,
//! which keeps generated row types and predicates on plain text.
//! Packed values are never empty, so `<> ''` searchability predicates and the
//! partial indexes that repeat them hold for both forms.
use rusqlite::Connection;
use rusqlite::functions::FunctionFlags;
use rusqlite::types::{Value, ValueRef};

/// Release builds pack only values large enough to shrink meaningfully. Debug
/// builds pack every non-empty value, so tests exercise every read path.
#[cfg(not(debug_assertions))]
const THRESHOLD: usize = 1024;
#[cfg(debug_assertions)]
const THRESHOLD: usize = 1;
const LEVEL: i32 = 3;
/// Larger than any stored message; bounds a corrupt frame's allocation.
const MAX_TEXT_BYTES: usize = 256 << 20;

pub fn pack(text: &str) -> Value {
    if text.len() < THRESHOLD || text.len() > MAX_TEXT_BYTES {
        return Value::Text(text.to_owned());
    }
    match zstd::bulk::compress(text.as_bytes(), LEVEL) {
        Ok(frame) if cfg!(debug_assertions) || frame.len() < text.len() => Value::Blob(frame),
        _ => Value::Text(text.to_owned()),
    }
}

pub fn unpack(frame: &[u8]) -> Result<String, &'static str> {
    let bytes =
        zstd::bulk::decompress(frame, MAX_TEXT_BYTES).map_err(|_| "corrupt packed silver text")?;
    String::from_utf8(bytes).map_err(|_| "packed silver text is not UTF-8")
}

/// Registers the SQL codec; every connection that reads or writes silver needs it.
///
/// # Errors
/// Fails when SQLite cannot register the functions.
pub fn register(connection: &Connection) -> rusqlite::Result<()> {
    // Innocuous: the schema's generated `content` columns call silver_text.
    let flags = FunctionFlags::SQLITE_UTF8
        | FunctionFlags::SQLITE_DETERMINISTIC
        | FunctionFlags::SQLITE_INNOCUOUS;
    connection.create_scalar_function("silver_pack", 1, flags, |ctx| {
        Ok(match ctx.get_raw(0) {
            ValueRef::Null => Value::Null,
            ValueRef::Text(text) => pack(std::str::from_utf8(text).map_err(user)?),
            _ => return Err(user("silver_pack takes text")),
        })
    })?;
    connection.create_scalar_function("silver_text", 1, flags, |ctx| {
        Ok(match ctx.get_raw(0) {
            ValueRef::Null => Value::Null,
            ValueRef::Text(text) => {
                Value::Text(std::str::from_utf8(text).map_err(user)?.to_owned())
            }
            ValueRef::Blob(frame) => Value::Text(unpack(frame).map_err(user)?),
            ValueRef::Integer(value) => Value::Integer(value),
            ValueRef::Real(value) => Value::Real(value),
        })
    })
}

fn user(error: impl std::fmt::Display) -> rusqlite::Error {
    rusqlite::Error::UserFunctionError(error.to_string().into())
}

#[cfg(test)]
mod tests {
    use super::{pack, register};
    use proptest::prelude::*;
    use rusqlite::types::Value;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        #[test]
        fn arbitrary_utf8_is_preserved_by_sql_storage(
            characters in proptest::collection::vec(any::<char>(), 0..2048),
        ) {
            let text: String = characters.into_iter().collect();
            let connection = rusqlite::Connection::open_in_memory().unwrap();
            register(&connection).unwrap();
            connection.execute_batch(
                "CREATE TABLE messages (content_packed,
                 content TEXT GENERATED ALWAYS AS (silver_text(content_packed)) VIRTUAL)",
            ).unwrap();
            connection.execute(
                "INSERT INTO messages(content_packed) VALUES(silver_pack(?1))", [&text],
            ).unwrap();
            let restored: String = connection.query_row(
                "SELECT content FROM messages", [], |row| row.get(0),
            ).unwrap();
            prop_assert_eq!(restored, text);
        }
    }

    #[test]
    fn packed_text_round_trips_through_sql_and_stays_distinguishable() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        register(&connection).unwrap();
        let long = "tool output line\n".repeat(200);
        for text in ["", "short", long.as_str()] {
            let (kind, back): (String, String) = connection
                .query_row(
                    "SELECT typeof(silver_pack(?1)), silver_text(silver_pack(?1))",
                    [text],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(back, text);
            // An empty value is never packed, so `<> ''` predicates stay exact.
            assert_eq!(
                kind == "text",
                text.is_empty() || !matches!(pack(text), Value::Blob(_))
            );
        }
        assert!(matches!(pack(&long), Value::Blob(_)));
        let null: Option<String> = connection
            .query_row("SELECT silver_text(silver_pack(NULL))", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(null, None);
        assert!(
            connection
                .query_row("SELECT silver_text(x'00ff')", [], |row| row
                    .get::<_, String>(0))
                .is_err()
        );
    }
}
