//! Readable evidence and mutation results; no parsing of serialized response maps.
use super::{json, line, safe, time};
use crate::core::models::{IndexResponse, ViewResponse};

pub(super) fn view(out: &mut String, value: &ViewResponse) -> Result<(), serde_json::Error> {
    line(
        out,
        &format!(
            "Mash view — {} · {} messages",
            safe(&value.id),
            value.messages.len()
        ),
    );
    if let Some(chat) = &value.chat {
        line(
            out,
            &format!(
                "Chat: {} · {} messages · omitted before: {} · omitted after: {}",
                safe(&chat.chat_id),
                chat.message_count,
                chat.omitted_before,
                chat.omitted_after
            ),
        );
    }
    if let Some(notice) = &value.notice {
        line(out, &format!("Notice: {}", safe(notice)));
    }
    for message in &value.messages {
        line(
            out,
            &format!(
                "\nMessage: {} · {} · ordinal={} · time (Unix ms): {}",
                safe(&message.id),
                message.role.as_str(),
                message.ordinal,
                time(message.created_at)
            ),
        );
        line(out, "Complete archived message:");
        for part in message.content.split('\n') {
            line(out, &format!("  | {}", safe(part)));
        }
        if !message.attachments.is_empty() {
            attachments(
                out,
                &crate::attachments::Listing {
                    message_id: message.id.clone(),
                    attachments: message.attachments.clone(),
                },
            )?;
        }
        tools(out, &message.tools);
    }
    Ok(())
}

/// Tool identities and outcomes; their inputs and outputs are in the message text.
fn tools(out: &mut String, value: &crate::tools::Activity) {
    let label = |value: Option<&str>| value.map_or_else(|| "unrecorded".to_owned(), safe);
    for call in &value.tool_calls {
        line(
            out,
            &format!(
                "Tool call: {} · call_id={}",
                label(call.name.as_deref()),
                label(call.call_id.as_deref())
            ),
        );
    }
    for result in &value.tool_results {
        let error = match result.is_error {
            Some(true) => "true",
            Some(false) => "false",
            None => "unrecorded",
        };
        line(
            out,
            &format!(
                "Tool result: call_id={} · is_error={error}",
                label(result.call_id.as_deref())
            ),
        );
    }
}

pub(super) fn publication(
    out: &mut String,
    value: &crate::shared::Publication,
) -> Result<(), serde_json::Error> {
    let value = value.summary();
    json(out, "Shared publication", &value.state)?;
    super::warning(out, "shared publication", value.error);
    Ok(())
}

pub(super) fn index(out: &mut String, value: &IndexResponse) -> Result<(), serde_json::Error> {
    line(
        out,
        &format!("Mash index — retry required: {}", value.needs_retry()),
    );
    line(
        out,
        &format!(
            "Indexed: {} chats, {} messages · searchable: {}",
            value.indexed_chats, value.indexed_messages, value.searchable_messages
        ),
    );
    line(
        out,
        &format!(
            "Changes: {} messages changed, {} removed · {} unchanged sources, {} checkpoint skips",
            value.changed_messages,
            value.removed_messages,
            value.unchanged_sources,
            value.checkpoint_skipped_sources
        ),
    );
    line(
        out,
        &format!(
            "Scanned: {} files · malformed records: {} · committed batches: {}",
            value.scanned_files, value.malformed_records, value.committed_batches
        ),
    );
    line(
        out,
        &format!(
            "Embeddings: {} · model inferences: {} ({} remote) · reused: {}",
            value.embeddings,
            value.model_inferences,
            value.remote_inferences,
            value.reused_embeddings
        ),
    );
    line(
        out,
        &format!(
            "Total time: {} ms · full scan: {} · views rebuild: {}",
            value.total_milliseconds, value.full, value.rebuild_local_views
        ),
    );
    json(out, "Mode", &value.realized_mode)?;
    json(out, "Silver", &value.silver)?;
    if !value.record_diagnostics.samples.is_empty() || value.record_diagnostics.omitted_sources > 0
    {
        json(
            out,
            "Rejected record diagnostics",
            &value.record_diagnostics,
        )?;
    }
    if let Some(shared) = &value.shared {
        publication(out, shared)?;
    }
    if value.needs_retry() {
        line(
            out,
            "Indexing is incomplete; fix reported source/publication failures and retry index.",
        );
    }
    Ok(())
}

fn counts(out: &mut String, value: &crate::export::HistoryCounts) {
    line(
        out,
        &format!(
            "Silver data: {} chats, {} messages · {} attachment files, {} bytes",
            value.chats, value.messages, value.attachment_files, value.attachment_bytes
        ),
    );
}

pub(super) fn exported(
    out: &mut String,
    value: &crate::export::Exported,
) -> Result<(), serde_json::Error> {
    line(
        out,
        &format!(
            "Mash silver exported to {}",
            safe(&value.directory.display().to_string())
        ),
    );
    line(out, &format!("Format version: {}", value.format_version));
    counts(out, &value.counts);
    json(out, "Silver", &value.silver)
}

pub(super) fn imported(
    out: &mut String,
    value: &crate::export::Imported,
) -> Result<(), serde_json::Error> {
    line(
        out,
        &format!(
            "Mash silver imported into {}",
            safe(&value.database.display().to_string())
        ),
    );
    counts(out, &value.counts);
    line(out, &format!("Search ready: {}", value.search_ready));
    json(out, "Silver", &value.silver)?;
    json(out, "Next action", &value.recommended_action)
}

pub(super) fn attachments(
    out: &mut String,
    value: &crate::attachments::Listing,
) -> Result<(), serde_json::Error> {
    line(
        out,
        &format!(
            "Mash attachments — message {} · {} files",
            safe(&value.message_id),
            value.attachments.len()
        ),
    );
    for attachment in &value.attachments {
        line(
            out,
            &format!(
                "  {} · {} · {}",
                safe(&attachment.id),
                safe(&attachment.name),
                safe(&attachment.media_type)
            ),
        );
        json(out, "    Availability", &attachment.availability)?;
        json(out, "    Bytes", &attachment.bytes)?;
        json(out, "    Digest", &attachment.digest)?;
        line(out, &format!("    Redacted: {}", attachment.redacted));
    }
    if !value.attachments.is_empty() {
        line(
            out,
            "Use get_attachment with these message/attachment IDs and the same shared node scope as this request.",
        );
    }
    Ok(())
}
