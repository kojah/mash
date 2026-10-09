//! Reads shared by every source in the silver object hierarchy.
use super::generated as sql;
use crate::AppError;
use crate::source::Ancestor;
use rusqlite::Connection;

/// The enclosing objects above a parent edge, nearest first.
pub(crate) fn ancestors(
    connection: &Connection,
    mut parent: Option<i64>,
) -> Result<Vec<Ancestor>, AppError> {
    let mut ancestors = Vec::new();
    while let Some(storage_id) = parent {
        // Silver rejects parent cycles; the bound only guards corrupt data.
        if ancestors.len() > 256 {
            return Err(AppError::database_data("object ancestry is too deep"));
        }
        let row = sql::ObjectAncestor::builder()
            .storage_id(storage_id)
            .build()
            .query_opt(connection)
            .map_err(AppError::database)?
            .ok_or_else(|| AppError::database_data("object ancestry is incomplete"))?;
        ancestors.push(Ancestor {
            id: row.id,
            role: super::connector_gold::parse_role(&row.object_role)
                .ok_or_else(|| AppError::database_data("stored object role is invalid"))?,
            kind: row.kind,
            title: row.title,
        });
        parent = row.parent_storage_id;
    }
    Ok(ancestors)
}
