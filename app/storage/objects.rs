//! Native silver topology validation.
use crate::AppError;
use rusqlite::Connection;

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
                "entry" => {
                    kind == format!("{source}:message")
                        && parent.as_deref() == Some(format!("{source}:chat").as_str())
                }
                _ => false,
            };
        if !conforms {
            violations.push(id);
        }
    }
    Ok(violations)
}
