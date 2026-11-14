//! Human-readable MCP discovery content, rendered from the same typed views as JSON.
use super::{Chat, Detail, Memories, Response, Search, SearchResults, Status};
use crate::search_results::{ChatMetadata, MessageReference, lookup::Lookup};
use serde::Serialize;

mod operations;

fn line(out: &mut String, text: &str) {
    out.push_str(text);
    out.push('\n');
}

fn safe(text: &str) -> String {
    text.chars().fold(String::new(), |mut out, character| {
        if character.is_control() {
            out.extend(character.escape_debug());
        } else {
            out.push(character);
        }
        out
    })
}

/// A Unix-millisecond time as a UTC date and minute, which a reader can
/// compare at sight; the exact value stays in the structured result.
fn time(value: Option<i64>) -> String {
    value
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map_or_else(
            || "unknown".into(),
            |time| time.format("%Y-%m-%dT%H:%MZ").to_string(),
        )
}

/// Characters of a chat title in a result heading. Many titles are a whole
/// first prompt; the structured result keeps them complete.
const TITLE_CHARS: usize = 160;

fn title(value: Option<&str>) -> String {
    let title = safe(value.unwrap_or("Untitled thread"));
    match title.char_indices().nth(TITLE_CHARS) {
        Some((end, _)) => format!("{}…", &title[..end]),
        None => title,
    }
}

fn json(out: &mut String, name: &str, value: &impl Serialize) -> Result<(), serde_json::Error> {
    line(out, &format!("{name}: {}", serde_json::to_string(value)?));
    Ok(())
}

fn warning(out: &mut String, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        line(out, &format!("Warning — {name}: {}", safe(value)));
    }
}

/// Where and when a chat happened. Its title is the result's heading.
fn metadata(out: &mut String, value: &ChatMetadata) {
    for (name, value) in [
        ("Project", &value.project),
        ("Repository", &value.repository),
    ] {
        if let Some(value) = value {
            line(out, &format!("  {name}: {}", safe(value)));
        }
    }
    if value.created_at.is_some() || value.updated_at.is_some() {
        line(
            out,
            &format!(
                "  Chat times (UTC): created {}, updated {}",
                time(value.created_at),
                time(value.updated_at)
            ),
        );
    }
}

fn evidence(out: &mut String, body: &str, truncated: bool) {
    line(out, "  Archived excerpt:");
    for part in body.split('\n') {
        line(out, &format!("    | {}", safe(part)));
    }
    if truncated {
        line(
            out,
            "  Excerpt truncated; use view for complete message text.",
        );
    }
}

fn anchors(
    out: &mut String,
    lookup: Option<&Lookup>,
    copies: &[MessageReference],
) -> Result<(), serde_json::Error> {
    // list_attachments takes the same message ID and shared value.
    if let Some(lookup) = lookup {
        json(out, "  lookup.view", &lookup.view)?;
    }
    if !copies.is_empty() {
        json(out, "  Copies", &copies)?;
    }
    Ok(())
}

fn origin(node: Option<&str>, lookup: Option<&Lookup>) -> String {
    match lookup {
        Some(lookup) => lookup.view.shared.as_deref().map_or_else(
            || {
                format!(
                    "local{}",
                    node.map_or_else(String::new, |node| format!(" / {}", safe(node)))
                )
            },
            |node| format!("shared / {}", safe(node)),
        ),
        None => node.map_or_else(
            || "index unspecified".into(),
            |node| format!("node {} (index unspecified)", safe(node)),
        ),
    }
}

fn thread(out: &mut String, number: usize, value: &Chat<'_>) -> Result<(), serde_json::Error> {
    line(
        out,
        &format!(
            "\n{number}. {} · {} · node {}",
            title(value.metadata.title.as_deref()),
            safe(&value.provider.to_string()),
            safe(value.node_id.unwrap_or("unspecified"))
        ),
    );
    line(out, &format!("  Chat: {}", safe(value.chat_id)));
    metadata(out, &value.metadata);
    if let Some(score) = value.score {
        line(out, &format!("  Chat score: {score}"));
    }
    if let Some(lookup) = value.lookup {
        json(out, "  First message view", &lookup.view)?;
    }
    for hit in &value.matches {
        // The message ID is in lookup.view below; without one it is given here.
        let identity = if hit.lookup.is_some() {
            String::new()
        } else {
            format!("{} · ", safe(hit.message_id))
        };
        line(
            out,
            &format!(
                "  Message: {identity}{} · {} · {}",
                hit.role.as_str(),
                origin(hit.node_id, hit.lookup),
                time(hit.created_at)
            ),
        );
        evidence(out, &hit.snippet, hit.snippet_truncated);
        anchors(out, hit.lookup, hit.copies)?;
    }
    Ok(())
}

fn search(out: &mut String, value: &Search<'_>) -> Result<(), serde_json::Error> {
    let count = match &value.results {
        SearchResults::Chats(rows) => rows.len(),
        SearchResults::Messages(rows) => rows.len(),
    };
    line(
        out,
        &format!("Mash search — {count} results for {}", safe(value.query)),
    );
    json(out, "Intent", &value.intent)?;
    json(out, "Mode", value.realized_mode)?;
    json(out, "Unit", &value.unit)?;
    line(out, &format!("Shared results included: {}", value.shared));
    warning(out, "shared search", value.shared_error);
    warning(out, "shared context", value.shared_context_error);
    for message in value.freshness_warnings {
        warning(out, "local indexing", Some(message));
    }
    if count == 0 {
        line(
            out,
            "No matches returned; this does not prove complete history or absence of chats.",
        );
    }
    match &value.results {
        SearchResults::Chats(rows) => {
            for (index, row) in rows.iter().enumerate() {
                thread(out, index + 1, row)?;
            }
        }
        SearchResults::Messages(rows) => {
            for (index, hit) in rows.iter().enumerate() {
                line(
                    out,
                    &format!(
                        "\n{}. {} · {} · {}",
                        index + 1,
                        safe(hit.id),
                        safe(&hit.provider.to_string()),
                        origin(hit.node_id, hit.lookup)
                    ),
                );
                line(
                    out,
                    &format!(
                        "  Chat: {} · role: {} · {}",
                        safe(hit.chat_id),
                        hit.role.as_str(),
                        time(hit.created_at)
                    ),
                );
                if let Some(title) = hit.metadata.title.as_deref() {
                    line(out, &format!("  Title: {}", self::title(Some(title))));
                }
                metadata(out, &hit.metadata);
                evidence(out, &hit.content, hit.content_truncated);
                anchors(out, hit.lookup, hit.copies)?;
            }
        }
    }
    if let Some(next) = value.next_offset {
        line(
            out,
            &format!("\nNext offset: {next}. Repeat the same search with this offset for more."),
        );
    }
    if let Some(diagnostics) = value.diagnostics {
        json(out, "Diagnostics", diagnostics)?;
    }
    Ok(())
}

fn coverage(
    out: &mut String,
    value: &crate::core::coverage::Report,
) -> Result<(), serde_json::Error> {
    line(out, "\nScoped coverage (retained data and recorded scans):");
    json(out, "  Requested scope", &value.scope)?;
    line(out, &format!("  Absence proven: {}", value.absence_proven));
    json(out, "  Limitations", &value.limitations)?;
    json(out, "  Unavailable nodes", &value.unavailable_nodes)?;
    for member in &value.members {
        line(out, &format!("  Node: {}", safe(&member.node_id)));
        json(out, "    Index", &member.index)?;
        match &member.retained {
            Some(rows) => json(out, "    Retained rows (message bounds in Unix ms)", rows)?,
            None => line(out, "    Scoped retained rows: unknown/unreported"),
        }
        if let Some(certificate) = &member.source_silver {
            json(out, "    Source silver", certificate)?;
        }
        match &member.providers {
            Some(scans) => {
                for scan in scans {
                    line(
                        out,
                        &format!(
                            "    Provider: {} · configured={} · rejected_sources={}",
                            scan.provider, scan.configured, scan.rejected_sources
                        ),
                    );
                    json(
                        out,
                        "      Recorded scan state (not historical completeness)",
                        &scan.state,
                    )?;
                }
            }
            None => line(out, "    Provider scan coverage: unknown/unreported"),
        }
    }
    Ok(())
}

fn status(out: &mut String, value: &Status<'_>) -> Result<(), serde_json::Error> {
    line(
        out,
        &format!("Mash status — local search ready: {}", value.ready),
    );
    line(
        out,
        &format!(
            "Local Qdrant: {} at {}",
            serde_json::to_string(&value.local_qdrant.state)?,
            safe(&value.local_qdrant.url)
        ),
    );
    warning(out, "local Qdrant", value.local_qdrant.message.as_deref());
    if let Some(watcher) = value
        .watcher
        .filter(|watcher| watcher.running == Some(false))
    {
        warning(
            out,
            "watcher",
            Some(&format!(
                "a watcher service is installed ({}) but no `mash index --watch` runs for this index, so history changes are not indexed automatically. Start it with its service manager (`launchctl kickstart gui/$(id -u)/dev.mash.index-watch` on macOS, `systemctl --user start mash-index-watch.service` on Linux) and check its log.",
                watcher.definition.display()
            )),
        );
    }
    json(out, "Artifacts", value.local_artifacts)?;
    json(out, "Installation", value.installation)?;
    json(out, "Query models", value.query_models)?;
    line(
        out,
        &format!(
            "Models installed: {} · dense support: {}",
            value.models_installed, value.semantic_support
        ),
    );
    line(
        out,
        &format!(
            "Retained: {} chats, {} messages · searchable: {} · embeddings: {}",
            value.chats, value.messages, value.searchable_messages, value.embeddings
        ),
    );
    json(out, "Mode", value.realized_mode)?;
    if let Some(action) = value.recommended_action {
        json(out, "Next action", action)?;
    }
    if let Some(silver) = value.silver {
        json(out, "Silver", silver)?;
    }
    line(out, "Readiness does not certify complete provider history.");
    warning(out, "shared status", value.shared_error);
    if let Some(publication) = &value.shared {
        json(out, "Shared publication", &publication.state)?;
        warning(out, "shared publication", publication.error);
    }
    if let Some(shared) = &value.shared_index {
        freshness(out, shared)?;
    }
    if let Some(value) = value.coverage {
        coverage(out, value)?;
    }
    Ok(())
}

fn freshness(
    out: &mut String,
    shared: &crate::shared::FreshnessSummary<'_>,
) -> Result<(), serde_json::Error> {
    line(out, &format!("Shared group: {}", safe(shared.group_id)));
    if let Some(unpublished) = shared.unpublished_nodes {
        json(out, "Unpublished nodes", &unpublished)?;
    }
    for node in &shared.nodes {
        line(
            out,
            &format!(
                "  Node {} · revision={} · {} chats, {} messages · lexical_current={} · views_current={}",
                safe(node.node_id),
                node.revision,
                node.chats,
                node.messages,
                node.lexical_current,
                node.views_current
            ),
        );
        json(out, "    Last synced", &node.last_synced_at)?;
        json(out, "    Last updated", &node.last_updated_at)?;
        if let Some(artifacts) = node.artifacts {
            json(out, "    Artifacts", artifacts)?;
        }
        if let Some(silver) = node.source_silver {
            json(out, "    Source silver", silver)?;
        }
    }
    Ok(())
}

pub fn render(
    response: &Response,
    detail: Detail,
    structured: &serde_json::Value,
) -> Result<String, serde_json::Error> {
    let mut out = String::new();
    match response {
        Response::RecordMutation(value) => json(&mut out, "Record operation", value)?,
        Response::Records(value) => records(&mut out, &Memories::from(value))?,
        Response::Record(value) => json(&mut out, "Record", value)?,
        Response::RecallRecords(value) => json(&mut out, "Record recall", value)?,
        Response::RecordCapture(value) => json(&mut out, "Memory capture", value)?,
        Response::RecordMatches(value) => json(&mut out, "Record suggestions", value)?,
        Response::ExtractionCheckpoint(value) => json(&mut out, "Extraction checkpoint", value)?,
        Response::ExtractRecords(value) => json(&mut out, "Extracted records", value)?,
        Response::Search(value) => search(&mut out, &Search::from(value))?,
        Response::Query(value) => json(&mut out, "Query", value)?,
        Response::ListChats(value) => list(&mut out, value)?,
        Response::DashboardCatalog(value) => json(&mut out, "Dashboard catalogue", value)?,
        Response::Status(value) => status(&mut out, &Status::from(value.as_ref()))?,
        Response::Installation(value) => json(&mut out, "Installation", value)?,
        Response::Init(value) => json(&mut out, "Init", value)?,
        Response::View(value) => operations::view(&mut out, value)?,
        Response::Index(value) => operations::index(&mut out, value)?,
        Response::Forget(value) => {
            line(
                &mut out,
                &format!(
                    "Mash forget — {} · forgotten: {}",
                    safe(&value.id),
                    value.forgotten
                ),
            );
            if let Some(error) = &value.local_views_error {
                line(
                    &mut out,
                    &format!("Local search refresh pending: {}", safe(error)),
                );
            }
            if let Some(shared) = &value.shared {
                operations::publication(&mut out, shared)?;
            }
        }
        Response::ModelsInstall(value) => {
            line(
                &mut out,
                &format!("Mash models — installed: {}", value.installed),
            );
            line(
                &mut out,
                &format!(
                    "Directory: {}",
                    safe(&value.model_directory.display().to_string())
                ),
            );
            line(
                &mut out,
                &format!(
                    "Embedding model: {} · dimensions: {}\nReranker: {} · files: {}",
                    safe(value.summary.embedding_model),
                    value.summary.embedding_dimensions,
                    safe(value.summary.reranker_model),
                    value.summary.files
                ),
            );
            json(&mut out, "Next action", &value.recommended_action)?;
        }
        Response::QdrantInstall(value) => json(&mut out, "Qdrant install", value)?,
        Response::QdrantUninstall(value) => json(&mut out, "Qdrant uninstall", value)?,
        Response::ExportSilver(value) => operations::exported(&mut out, value)?,
        Response::ImportSilver(value) => operations::imported(&mut out, value)?,
        Response::ListAttachments(value) => operations::attachments(&mut out, value)?,
        Response::GetAttachment(value) => operations::attachments(
            &mut out,
            &crate::attachments::Listing {
                message_id: value.message_id.clone(),
                attachments: vec![value.attachment.clone()],
            },
        )?,
    }
    if matches!(detail, Detail::Extended)
        && matches!(
            response,
            Response::Search(_)
                | Response::ListChats(_)
                | Response::Status(_)
                | Response::Records(_)
        )
    {
        line(&mut out, "\nFull details (JSON):");
        line(&mut out, &structured.to_string());
    }
    Ok(out)
}

pub fn error(value: &crate::AppError) -> Result<String, serde_json::Error> {
    let mut out = format!("Mash error — {}\n", safe(&value.error.message));
    json(&mut out, "Kind", &value.error.kind)?;
    line(&mut out, &format!("Retryable: {}", value.error.retryable));
    if let Some(database) = &value.error.database {
        json(&mut out, "Database", database)?;
    }
    if let Some(action) = &value.error.recommended_action {
        json(&mut out, "Next action", action)?;
    }
    Ok(out)
}

fn scope(value: &crate::records::Scope) -> String {
    use crate::records::Scope;
    match value {
        Scope::Personal => "personal".into(),
        Scope::Repository { identity } => format!("repository {}", safe(identity)),
        Scope::LocalProject { path } => format!("project {}", safe(path)),
        Scope::Chat { id } => format!("chat {}", safe(id)),
    }
}

fn records(out: &mut String, value: &Memories<'_>) -> Result<(), serde_json::Error> {
    line(
        out,
        &format!("Mash records — {} results", value.records.len()),
    );
    for (index, record) in value.records.iter().enumerate() {
        line(
            out,
            &format!(
                "\n{}. {} · {} · {}",
                index + 1,
                record.kind,
                safe(&record.state),
                scope(record.scope)
            ),
        );
        line(out, &format!("  Statement: {}", safe(record.statement)));
        if !record.rationale.is_empty() {
            line(out, &format!("  Why: {}", safe(record.rationale)));
        }
        line(
            out,
            &format!(
                "  Recorded: {} by {} · revision {} · {}",
                time(Some(record.recorded_at)),
                safe(record.recorded_by),
                record.revision,
                serde_json::to_string(&record.review)?.trim_matches('"')
            ),
        );
        if record.evidence_stale {
            line(
                out,
                "  Its source messages have changed since it was recorded.",
            );
        }
        if record.conflicted {
            line(out, "  Conflicts with another record; see get_record.");
        }
        line(out, &format!("  Record: {}", safe(record.id)));
        if let Some(node) = record.node_id {
            line(out, &format!("  Node: {}", safe(node)));
        }
        for source in &record.evidence {
            line(
                out,
                &format!(
                    "  Source: chat {} · message {}",
                    safe(source.chat_id),
                    safe(source.message_id)
                ),
            );
        }
    }
    if value.records.is_empty() {
        line(
            out,
            "No records returned; this does not prove that nothing was decided.",
        );
    }
    if let Some(next) = value.next_offset {
        line(
            out,
            &format!("\nNext offset: {next}. Repeat list_records with this offset for more."),
        );
    }
    if let Some(cursor) = value.next_cursor {
        json(out, "Next cursor", cursor)?;
    }
    line(
        out,
        "get_record has each record's quotes, history and links.",
    );
    Ok(())
}

fn list(out: &mut String, value: &crate::core::chats::Response) -> Result<(), serde_json::Error> {
    line(
        out,
        &format!(
            "Mash chats — {} results · shared included: {}",
            value.chats.len(),
            value.shared
        ),
    );
    warning(out, "shared listing", value.shared_error.as_deref());
    for (index, row) in value.chats.iter().enumerate() {
        thread(out, index + 1, &Chat::from(row))?;
    }
    json(out, "Next offset", &value.next_offset)?;
    if value.next_offset.is_some() {
        line(
            out,
            "Continue list_chats with the same filters/order and this offset.",
        );
    }
    if value.chats.is_empty() {
        line(
            out,
            "No chats returned; this does not prove complete history or absence of chats.",
        );
    }
    if let Some(diagnostics) = &value.diagnostics {
        json(out, "Diagnostics", diagnostics)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn readable_freshness_keeps_revision_times_and_unpublished_nodes() {
        let value: crate::shared::Freshness = serde_json::from_value(json!({
            "group_id":"group", "unpublished_nodes":["missing-peer"], "nodes":[{
                "node_id":"peer", "instance":"instance", "revision":4,
                "last_synced_at":"2026-10-08T12:00:00Z", "last_updated_at":null,
                "chats":2,"messages":20,"lexical_current":true,"views_current":false
            }]
        }))
        .unwrap();
        let mut out = String::new();
        freshness(&mut out, &value.summary()).unwrap();
        assert!(out.contains("missing-peer"));
        assert!(out.contains("revision=4"));
        assert!(out.contains("views_current=false"));
        assert!(out.contains("Last synced: \"2026-10-08T12:00:00Z\""));
        assert!(out.contains("Last updated: null"));
    }

    #[test]
    fn unknown_scoped_shared_coverage_stays_unknown() {
        let value = crate::core::coverage::Report {
            scope: crate::core::coverage::Scope::default(),
            absence_proven: false,
            limitations: vec![crate::core::coverage::Limitation::SharedProviderScanStateUnreported],
            unavailable_nodes: vec!["unavailable".into()],
            members: vec![crate::core::coverage::Member {
                node_id: "peer".into(),
                index: crate::core::coverage::Index::Shared,
                source_silver: None,
                retained: None,
                providers: None,
            }],
        };
        let mut out = String::new();
        coverage(&mut out, &value).unwrap();
        assert!(out.contains("Scoped retained rows: unknown/unreported"));
        assert!(out.contains("Provider scan coverage: unknown/unreported"));
        assert!(out.contains("unavailable"));
        assert!(out.contains("Absence proven: false"));
    }

    #[test]
    fn source_attribution_does_not_invent_shared_lookup() {
        let lookup = Lookup {
            view: crate::search_results::lookup::View {
                id: "id".into(),
                shared: None,
            },
            list_attachments: crate::search_results::lookup::Attachments {
                message_id: "id".into(),
                shared: None,
            },
        };
        assert_eq!(origin(Some("own-node"), Some(&lookup)), "local / own-node");
        assert_eq!(origin(Some("peer"), None), "node peer (index unspecified)");
        assert_eq!(origin(None, None), "index unspecified");
    }
}
