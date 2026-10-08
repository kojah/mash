//! Human-readable MCP discovery content, rendered from the same typed views as JSON.
use super::{Detail, Response, Search, SearchResults, Status, Thread};
use crate::search_results::{MessageReference, ThreadMetadata, lookup::Lookup};
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

fn time(value: Option<i64>) -> String {
    value.map_or_else(|| "unknown".into(), |value| value.to_string())
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

fn metadata(out: &mut String, value: &ThreadMetadata) {
    for (name, value) in [
        ("Title", &value.title),
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
                "  Thread times (Unix ms): created={}, updated={}",
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
    if let Some(lookup) = lookup {
        json(out, "  lookup.view", &lookup.view)?;
        json(out, "  lookup.list_attachments", &lookup.list_attachments)?;
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

fn thread(out: &mut String, number: usize, value: &Thread<'_>) -> Result<(), serde_json::Error> {
    line(
        out,
        &format!(
            "\n{number}. {} · {} · node {}",
            safe(value.metadata.title.as_deref().unwrap_or("Untitled thread")),
            value.provider,
            safe(value.node_id.unwrap_or("unspecified"))
        ),
    );
    line(out, &format!("  Thread: {}", safe(value.conversation_id)));
    metadata(out, &value.metadata);
    if let Some(score) = value.score {
        line(out, &format!("  Thread score: {score}"));
    }
    for hit in &value.matches {
        line(
            out,
            &format!(
                "  Message: {} · {} · {} · time (Unix ms): {}",
                safe(hit.message_id),
                hit.role.as_str(),
                origin(hit.node_id, hit.lookup),
                time(hit.created_at)
            ),
        );
        if let Some(matched) = hit.lexical_match {
            json(out, "  Lexical match", matched)?;
        }
        evidence(out, &hit.snippet, hit.snippet_truncated);
        anchors(out, hit.lookup, hit.copies)?;
    }
    Ok(())
}

fn search(out: &mut String, value: &Search<'_>) -> Result<(), serde_json::Error> {
    let count = match &value.results {
        SearchResults::Threads(rows) => rows.len(),
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
    if count == 0 {
        line(
            out,
            "No matches returned; this does not prove complete history or absence of conversations.",
        );
    }
    match &value.results {
        SearchResults::Threads(rows) => {
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
                        hit.provider,
                        origin(hit.node_id, hit.lookup)
                    ),
                );
                line(
                    out,
                    &format!(
                        "  Thread: {} · role: {} · time (Unix ms): {}",
                        safe(hit.conversation_id),
                        hit.role.as_str(),
                        time(hit.created_at)
                    ),
                );
                metadata(out, &hit.metadata);
                if let Some(matched) = hit.lexical_match {
                    json(out, "  Lexical match", matched)?;
                }
                evidence(out, &hit.content, hit.content_truncated);
                anchors(out, hit.lookup, hit.copies)?;
            }
        }
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
    json(out, "Artifacts", value.local_artifacts)?;
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
            "Retained: {} threads, {} messages · searchable: {} · embeddings: {}",
            value.conversations, value.messages, value.searchable_messages, value.embeddings
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
                "  Node {} · revision={} · {} threads, {} messages · lexical_current={} · gold_current={}",
                safe(node.node_id),
                node.revision,
                node.conversations,
                node.messages,
                node.lexical_current,
                node.gold_current
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

pub(crate) fn render(
    response: &Response,
    detail: Detail,
    structured: &serde_json::Value,
) -> Result<String, serde_json::Error> {
    let mut out = String::new();
    match response {
        Response::Search(value) => search(&mut out, &Search::from(value))?,
        Response::ListThreads(value) => list(&mut out, value)?,
        Response::Status(value) => status(&mut out, &Status::from(value.as_ref()))?,
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
            Response::Search(_) | Response::ListThreads(_) | Response::Status(_)
        )
    {
        line(&mut out, "\nFull details (JSON):");
        line(&mut out, &structured.to_string());
    }
    Ok(out)
}

pub(crate) fn error(value: &crate::AppError) -> Result<String, serde_json::Error> {
    let mut out = format!("Mash error — {}\n", safe(&value.error.message));
    json(&mut out, "Kind", &value.error.kind)?;
    line(&mut out, &format!("Retryable: {}", value.error.retryable));
    if let Some(action) = &value.error.recommended_action {
        json(&mut out, "Next action", action)?;
    }
    Ok(out)
}

fn list(out: &mut String, value: &crate::core::threads::Response) -> Result<(), serde_json::Error> {
    line(
        out,
        &format!(
            "Mash threads — {} results · shared included: {}",
            value.page.threads.len(),
            value.shared
        ),
    );
    warning(out, "shared listing", value.shared_error.as_deref());
    for (index, row) in value.page.threads.iter().enumerate() {
        thread(
            out,
            index + 1,
            &Thread {
                conversation_id: &row.conversation_id,
                provider: row.provider,
                node_id: Some(&row.node_id),
                metadata: super::metadata(&row.metadata),
                score: None,
                matches: Vec::new(),
            },
        )?;
    }
    json(out, "Next offset", &value.page.next_offset)?;
    if value.page.next_offset.is_some() {
        line(
            out,
            "Continue list_threads with the same filters/order and this offset.",
        );
    }
    if value.page.threads.is_empty() {
        line(
            out,
            "No threads returned; this does not prove complete history or absence of conversations.",
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
                "conversations":2,"messages":20,"lexical_current":true,"gold_current":false
            }]
        }))
        .unwrap();
        let mut out = String::new();
        freshness(&mut out, &value.summary()).unwrap();
        assert!(out.contains("missing-peer"));
        assert!(out.contains("revision=4"));
        assert!(out.contains("gold_current=false"));
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
