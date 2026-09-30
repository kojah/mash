use std::fs::{self, FileTimes};
use std::io::Write;

use super::parsers::{capture_metadata, parse_claude, parse_codex, stable_id};
use super::*;
use std::time::UNIX_EPOCH;

mod indexing;
mod parsing;

fn write_claude_session(path: &Path, session: &str, message: &str) {
    fs::write(
        path,
        format!(
            "{{\"type\":\"user\",\"sessionId\":\"{session}\",\"uuid\":\"{session}-m1\",\"message\":{{\"role\":\"user\",\"content\":\"{message}\"}}}}\n"
        ),
    )
    .expect("Claude fixture");
}

fn test_conversation(
    id: &str,
    provider: Provider,
    source_path: PathBuf,
    content: &str,
) -> Conversation {
    Conversation {
        id: id.to_owned(),
        provider,
        source_path,
        project: None,
        repository: None,
        title: None,
        created_at: None,
        updated_at: None,
        messages: vec![NormalizedMessage {
            id: format!("{id}-message"),
            ordinal: 0,
            role: crate::role::Role::User,
            content: content.to_owned(),
            search_projection: None,
            created_at: None,
        }],
    }
}

fn empty_summary() -> IndexSummary {
    IndexSummary {
        scanned_files: 0,
        malformed_records: 0,
        changed_messages: 0,
        removed_messages: 0,
        unchanged_sources: 0,
        checkpoint_skipped_sources: 0,
        tombstoned_sources: 0,
        purged_conversations: 0,
        committed_batches: 0,
        discovered_bytes: 0,
        processed_bytes: 0,
        processed_files: 0,
    }
}

fn set_modified(path: &Path, modified: SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .expect("open fixture")
        .set_times(FileTimes::new().set_modified(modified))
        .expect("set fixture modification time");
}

fn parse_with_late_failure(path: &Path) -> Result<ParsedFile, AppError> {
    if path.file_name().and_then(|name| name.to_str()) == Some("failure.jsonl") {
        std::thread::sleep(Duration::from_millis(50));
        return Err(AppError::io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "source disappeared",
        )));
    }
    parse_claude(path)
}

fn parse_then_append(path: &Path) -> Result<ParsedFile, AppError> {
    let parsed = parse_claude(path)?;
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(b"\n"))
        .map_err(AppError::io)?;
    Ok(parsed)
}
