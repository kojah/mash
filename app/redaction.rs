mod detection;
pub(crate) use detection::{projection, text};

pub(crate) fn metadata(metadata: &mut crate::search_results::ThreadMetadata) {
    let mut changed = false;
    for value in [
        &mut metadata.title,
        &mut metadata.project,
        &mut metadata.repository,
    ]
    .into_iter()
    .flatten()
    {
        let safe = text(value);
        changed |= safe != *value;
        *value = safe;
    }
    if changed {
        metadata.enrichment = None;
    }
}

pub(crate) fn conversation(record: &mut crate::shared::protocol::ConversationRecord) {
    let mut changed = false;
    for value in [
        &mut record.title,
        &mut record.project,
        &mut record.repository,
    ]
    .into_iter()
    .flatten()
    {
        let safe = text(value);
        changed |= safe != *value;
        *value = safe;
    }
    if changed {
        record.enrichment = None;
    }
}

pub(crate) fn batch(batch: &mut crate::shared::protocol::Batch) {
    use crate::shared::protocol::Batch;
    match batch {
        Batch::Conversations { records } => records.iter_mut().for_each(conversation),
        Batch::Messages { records } => {
            for record in records {
                record.content = text(&record.content);
                record.projection = text(&record.projection);
                attachments(&mut record.attachments);
            }
        }
        Batch::Tombstones { .. } => {}
    }
}

pub(crate) fn attachments(records: &mut [crate::attachments::Metadata]) {
    for record in records {
        record.name = text(&record.name);
        record.media_type = text(&record.media_type);
    }
}

#[cfg(test)]
mod tests;
