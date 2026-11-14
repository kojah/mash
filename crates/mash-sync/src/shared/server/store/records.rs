//! Owner-specific reads within the authenticated node group, without inference.
use super::{Identity, Store, silver_store};
use crate::{
    AppError,
    records::{self, Cursor, Get, List, Page, Record},
};

impl Store {
    pub async fn list_records(
        &self,
        identity: &Identity,
        mut input: List,
    ) -> Result<Page, AppError> {
        let limit = input.limit.unwrap_or(20);
        if !(1..=100).contains(&limit) || input.offset > 100_000 {
            return Err(AppError::usage(
                "record limit must be 1..100 and offset at most 100000",
            ));
        }
        if let Some(node) = &input.node_id {
            records::bounded(node, 4096, "node ID")?;
        }
        if let Some(cursor) = &input.cursor {
            records::bounded(&cursor.node_id, 4096, "cursor node ID")?;
            if cursor.offset > 100_000 || input.offset != 0 {
                return Err(AppError::usage(
                    "use the shared cursor without a separate offset",
                ));
            }
            if input
                .node_id
                .as_ref()
                .is_some_and(|node| node != &cursor.node_id)
            {
                return Err(AppError::usage(
                    "record cursor does not match the selected node",
                ));
            }
        }
        let nodes = self
            .heads(&identity.group_id)
            .await?
            .into_keys()
            .filter(|node| {
                input
                    .node_id
                    .as_ref()
                    .is_none_or(|selected| selected == node)
            })
            .collect::<Vec<_>>();
        let selected = if let Some(cursor) = &input.cursor {
            nodes
                .iter()
                .position(|node| node == &cursor.node_id)
                .ok_or_else(|| AppError::usage("record cursor node is no longer available"))?
        } else {
            0
        };
        let Some(node) = nodes.get(selected).cloned() else {
            if input.node_id.is_some() {
                return Err(AppError::usage(
                    "record node is not available in this group",
                ));
            }
            return Ok(Page {
                records: Vec::new(),
                next_offset: None,
                next_cursor: None,
            });
        };
        if let Some(cursor) = input.cursor.take() {
            input.offset = cursor.offset;
        }
        let scope = silver_store::node_identity(&identity.group_id, &node);
        let mut page = self
            .read(&scope, move |connection| {
                crate::storage::records::list(connection, &input)
            })
            .await?;
        for record in &mut page.records {
            record.node_id = Some(node.clone());
        }
        page.next_cursor = page
            .next_offset
            .take()
            .map(|offset| Cursor {
                node_id: node,
                offset,
            })
            .or_else(|| {
                nodes.get(selected + 1).map(|node| Cursor {
                    node_id: node.clone(),
                    offset: 0,
                })
            });
        Ok(page)
    }

    pub async fn get_record(&self, identity: &Identity, input: Get) -> Result<Record, AppError> {
        let node = input
            .node_id
            .clone()
            .unwrap_or_else(|| identity.node_id.clone());
        records::bounded(&node, 4096, "node ID")?;
        if !self.heads(&identity.group_id).await?.contains_key(&node) {
            return Err(AppError::usage(
                "record node is not available in this group",
            ));
        }
        let scope = silver_store::node_identity(&identity.group_id, &node);
        let mut record = self
            .read(&scope, move |connection| {
                let transaction = connection
                    .unchecked_transaction()
                    .map_err(AppError::database)?;
                crate::storage::records::read_page(&transaction, &input)
            })
            .await?;
        record.node_id = Some(node);
        Ok(record)
    }
}
