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
            role: super::connector_views::parse_role(&row.object_role)
                .ok_or_else(|| AppError::database_data("stored object role is invalid"))?,
            kind: row.kind,
            title: row.title,
        });
        parent = row.parent_storage_id;
    }
    Ok(ancestors)
}

/// Native objects that break their provider's declared topology.
pub(crate) fn native_violations(connection: &Connection) -> Result<Vec<String>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT o.id, o.source, o.object_role, o.kind, p.kind FROM objects o
               LEFT JOIN objects p ON p.storage_id = o.parent_storage_id
              WHERE o.origin = 'provider'",
        )
        .map_err(AppError::database)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(AppError::database)?;
    let mut violations = Vec::new();
    for row in rows {
        let (id, source, role, kind, parent) = row.map_err(AppError::database)?;
        let conforms = source.parse::<crate::provider::Provider>().is_ok()
            && match role.as_str() {
                "container" => kind == format!("{source}:chat") && parent.is_none(),
                "segment" => {
                    kind == format!("{source}:turn")
                        && parent.as_deref() == Some(format!("{source}:chat").as_str())
                }
                "entry" => {
                    kind == format!("{source}:message")
                        && parent.as_deref().is_some_and(|parent| {
                            parent == format!("{source}:chat") || parent == format!("{source}:turn")
                        })
                }
                _ => false,
            };
        if !conforms {
            violations.push(id);
        }
    }
    Ok(violations)
}
