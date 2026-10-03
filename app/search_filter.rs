//! Gold query models: selection and boolean predicates over silver metadata.
use crate::provider::Provider;
use rmcp::schemars;
use serde::{Deserialize, Serialize};

use crate::AppError;

#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Selection {
    /// Provider alternatives; omitted or empty selects all providers.
    pub(crate) provider: Vec<Provider>,
    pub(crate) days: Option<u32>,
    pub(crate) node: Vec<String>,
    pub(crate) role: Vec<crate::role::Role>,
    /// Canonical thread IDs from thread search; omitted or empty selects all threads.
    pub(crate) conversation: Vec<String>,
    /// Exact recorded working directory; repeat for alternatives.
    pub(crate) project: Vec<String>,
    /// Exact Git origin URL; repeat for alternatives.
    pub(crate) repository: Vec<String>,
    /// Inclusive Unix timestamp in seconds.
    pub(crate) after: Option<i64>,
    /// Exclusive Unix timestamp in seconds.
    pub(crate) before: Option<i64>,
    /// Boolean expression using all/any/not and metadata predicates.
    pub(crate) filter: Option<Filter>,
}

#[derive(Clone)]
pub(crate) struct Scope {
    pub(crate) filter: Filter,
    pub(crate) node: String,
}

/// Predicate names only; diagnostics never echo filter values or the query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Field {
    Provider,
    Node,
    Role,
    Conversation,
    Project,
    Repository,
    After,
    Before,
}

impl Selection {
    pub(crate) fn fields(&self) -> Vec<Field> {
        let mut fields = std::collections::BTreeSet::new();
        for (present, field) in [
            (!self.provider.is_empty(), Field::Provider),
            (!self.node.is_empty(), Field::Node),
            (!self.role.is_empty(), Field::Role),
            (!self.conversation.is_empty(), Field::Conversation),
            (!self.project.is_empty(), Field::Project),
            (!self.repository.is_empty(), Field::Repository),
            (self.after.is_some() || self.days.is_some(), Field::After),
            (self.before.is_some(), Field::Before),
        ] {
            if present {
                fields.insert(field);
            }
        }
        if let Some(filter) = &self.filter {
            filter.fields(&mut fields);
        }
        fields.into_iter().collect()
    }

    pub(crate) fn resolve(self, node: &str) -> Result<Scope, AppError> {
        let mut all = Vec::new();
        if !self.provider.is_empty() {
            all.push(Filter::Provider(self.provider));
        }
        if !self.role.is_empty() {
            all.push(Filter::Role(self.role));
        }
        for (values, constructor) in [
            (self.node, Filter::Node as fn(Vec<String>) -> Filter),
            (self.conversation, Filter::Conversation),
            (self.project, Filter::Project),
            (self.repository, Filter::Repository),
        ] {
            if !values.is_empty() {
                all.push(constructor(values));
            }
        }
        if let Some(days) = self.days {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| AppError::usage("invalid system clock"))?
                .as_secs();
            let cutoff = now.saturating_sub(u64::from(days) * 86400);
            all.push(Filter::After(
                i64::try_from(cutoff).map_err(|_| AppError::usage("invalid system clock"))?,
            ));
        }
        if let Some(after) = self.after {
            all.push(Filter::After(after));
        }
        if let Some(before) = self.before {
            all.push(Filter::Before(before));
        }
        if let Some(filter) = self.filter {
            all.push(filter);
        }
        let filter = Filter::All(all);
        filter.validate()?;
        Ok(Scope {
            filter,
            node: node.to_owned(),
        })
    }
}

/// Values within one field are alternatives. Compose fields with all, any, and not.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Filter {
    All(Vec<Self>),
    Any(Vec<Self>),
    Not(Box<Self>),
    Node(Vec<String>),
    Provider(Vec<Provider>),
    Role(Vec<crate::role::Role>),
    Conversation(Vec<String>),
    Project(Vec<String>),
    Repository(Vec<String>),
    /// Inclusive Unix timestamp in seconds.
    After(i64),
    /// Exclusive Unix timestamp in seconds.
    Before(i64),
}

impl Filter {
    fn fields(&self, fields: &mut std::collections::BTreeSet<Field>) {
        let field = match self {
            Self::All(children) | Self::Any(children) => {
                for child in children {
                    child.fields(fields);
                }
                return;
            }
            Self::Not(child) => return child.fields(fields),
            Self::Node(_) => Field::Node,
            Self::Provider(_) => Field::Provider,
            Self::Role(_) => Field::Role,
            Self::Conversation(_) => Field::Conversation,
            Self::Project(_) => Field::Project,
            Self::Repository(_) => Field::Repository,
            Self::After(_) => Field::After,
            Self::Before(_) => Field::Before,
        };
        fields.insert(field);
    }

    pub(crate) fn validate(&self) -> Result<(), AppError> {
        self.validate_at(0, &mut 0)
    }

    fn validate_at(&self, depth: usize, count: &mut usize) -> Result<(), AppError> {
        *count += 1;
        if depth > 8 || *count > 128 {
            return Err(AppError::usage(
                "search filter exceeds depth 8 or 128 expressions",
            ));
        }
        match self {
            Self::All(children) | Self::Any(children) => {
                for child in children {
                    child.validate_at(depth + 1, count)?;
                }
            }
            Self::Not(child) => child.validate_at(depth + 1, count)?,
            Self::After(time) | Self::Before(time) => {
                if !(0..=253_402_300_799).contains(time) {
                    return Err(AppError::usage(
                        "filter timestamps must be between 1970 and 9999",
                    ));
                }
            }
            _ => {
                let values = self.values();
                if values.len() > 100
                    || values.iter().any(|s| {
                        s.trim().is_empty() || s.len() > 4096 || s.chars().any(char::is_control)
                    })
                {
                    return Err(AppError::usage(
                        "filter lists accept up to 100 nonempty values of at most 4096 bytes",
                    ));
                }
            }
        }
        Ok(())
    }

    fn values(&self) -> Vec<&str> {
        match self {
            Self::Node(v) | Self::Conversation(v) | Self::Project(v) | Self::Repository(v) => {
                v.iter().map(String::as_str).collect()
            }
            Self::Provider(v) => v.iter().map(|p| p.as_str()).collect(),
            Self::Role(v) => v.iter().map(crate::role::Role::as_str).collect(),
            _ => Vec::new(),
        }
    }

    /// Identifiers come only from this closed enum; literals are SQL quoted.
    /// COALESCE preserves the filter tree's Boolean semantics for absent metadata.
    pub(crate) fn sql(&self, local_node: Option<&str>) -> String {
        match self {
            Self::All(v) => join_sql(v, " AND ", "TRUE", local_node),
            Self::Any(v) => join_sql(v, " OR ", "FALSE", local_node),
            Self::Not(v) => format!("(NOT {})", v.sql(local_node)),
            Self::After(v) => format!("COALESCE(m.created_at >= {}, FALSE)", v * 1000),
            Self::Before(v) => format!("COALESCE(m.created_at < {}, FALSE)", v * 1000),
            _ => {
                let column = match self {
                    Self::Node(_) => local_node.map_or_else(|| "m.node_id".to_owned(), quote),
                    Self::Provider(_) => "c.provider".to_owned(),
                    Self::Role(_) => "m.role".to_owned(),
                    Self::Conversation(_) => "m.conversation_id".to_owned(),
                    Self::Project(_) => "c.project".to_owned(),
                    Self::Repository(_) => "c.repository".to_owned(),
                    _ => unreachable!(),
                };
                let values = self.values();
                if values.is_empty() {
                    return "FALSE".to_owned();
                }
                format!(
                    "COALESCE({column} IN ({}), FALSE)",
                    values
                        .iter()
                        .map(|v| quote(v))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
        }
    }
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn join_sql(children: &[Filter], join: &str, empty: &str, node: Option<&str>) -> String {
    if children.is_empty() {
        empty.to_owned()
    } else {
        format!(
            "({})",
            children
                .iter()
                .map(|f| f.sql(node))
                .collect::<Vec<_>>()
                .join(join)
        )
    }
}
pub(crate) fn timestamp(value: &str) -> Result<i64, AppError> {
    let time = chrono::DateTime::parse_from_rfc3339(value)
        .map(|t| t.timestamp())
        .or_else(|_| {
            chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").map(|d| {
                d.and_hms_opt(0, 0, 0)
                    .expect("midnight")
                    .and_utc()
                    .timestamp()
            })
        })
        .map_err(|_| AppError::usage("dates must be YYYY-MM-DD or RFC3339 timestamps"))?;
    Filter::After(time).validate()?;
    Ok(time)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_fields_are_deduplicated_across_boolean_and_shorthand_filters() {
        let selection: Selection = serde_json::from_str(
            r#"{
            "provider":["codex"], "days":0,
            "filter":{"all":[{"provider":[]},{"any":[{"role":["user"]},
                {"not":{"project":["private-filter-canary"]}}]},{"after":1}]}
        }"#,
        )
        .unwrap();
        assert_eq!(
            selection.fields(),
            [Field::Provider, Field::Role, Field::Project, Field::After]
        );
        assert!(
            !serde_json::to_string(&selection.fields())
                .unwrap()
                .contains("private-filter-canary")
        );
        assert_eq!(Selection::default().fields(), [] as [Field; 0]);
    }

    #[test]
    fn expressions_validate_and_compile_without_interpolating_query_syntax() {
        let f: Filter = serde_json::from_str(r#"{"all":[{"any":[{"provider":["codex"]},{"role":["user"]}]},{"not":{"project":["x') OR TRUE --"]}},{"after":1}]}"#).unwrap();
        f.validate().unwrap();
        assert!(f.sql(Some("local")).contains("'x'') OR TRUE --'"));
        assert_eq!(
            Filter::Node(vec!["local".into()]).sql(Some("local")),
            "COALESCE('local' IN ('local'), FALSE)"
        );
        assert_eq!(timestamp("2026-09-30").unwrap(), 1_790_726_400);
        assert!(timestamp("yesterday").is_err());
        assert!(serde_json::from_str::<Filter>(r#"{"provider":["invalid"]}"#).is_err());
        assert!(Filter::Project(vec!["nul\0".into()]).validate().is_err());
        let mut deep = Filter::All(vec![]);
        for _ in 0..10 {
            deep = Filter::Not(Box::new(deep));
        }
        assert!(deep.validate().is_err());
        assert!(serde_json::from_str::<Filter>(r#"{"project":["x"],"role":["user"]}"#).is_err());
    }
}
