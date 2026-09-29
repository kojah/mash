use rmcp::schemars;
use serde::{Deserialize, Serialize};

use crate::AppError;

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct Arguments {
    #[arg(long)]
    provider: Vec<String>,
    #[arg(long)]
    days: Option<u32>,
    #[arg(long)]
    node: Vec<String>,
    #[arg(long)]
    role: Vec<String>,
    #[arg(long)]
    conversation: Vec<String>,
    /// Exact recorded working directory; repeat for alternatives.
    #[arg(long)]
    project: Vec<String>,
    /// Exact Git origin URL; repeat for alternatives.
    #[arg(long)]
    repository: Vec<String>,
    /// Inclusive date or RFC3339 timestamp.
    #[arg(long)]
    after: Option<String>,
    /// Exclusive date or RFC3339 timestamp.
    #[arg(long)]
    before: Option<String>,
    /// JSON Boolean expression using all/any/not and metadata predicates.
    #[arg(long)]
    filter: Option<String>,
}

#[derive(Clone)]
pub(crate) struct Scope {
    pub(crate) filter: Filter,
    pub(crate) node: String,
}

impl Arguments {
    pub(crate) fn resolve(self, node: &str) -> Result<Scope, AppError> {
        let mut all = Vec::new();
        for (values, constructor) in [
            (self.provider, Filter::Provider as fn(Vec<String>) -> Filter),
            (self.node, Filter::Node),
            (self.role, Filter::Role),
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
            all.push(Filter::After(timestamp(&after)?));
        }
        if let Some(before) = self.before {
            all.push(Filter::Before(timestamp(&before)?));
        }
        if let Some(filter) = self.filter {
            if filter.len() > 65536 {
                return Err(AppError::usage("search filter exceeds 64 KiB"));
            }
            all.push(
                serde_json::from_str(&filter)
                    .map_err(|_| AppError::usage("invalid structured search filter"))?,
            );
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
    Provider(Vec<String>),
    Role(Vec<String>),
    Conversation(Vec<String>),
    Project(Vec<String>),
    Repository(Vec<String>),
    /// Inclusive Unix timestamp in seconds.
    After(i64),
    /// Exclusive Unix timestamp in seconds.
    Before(i64),
}

impl Filter {
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
                if matches!(self, Self::Provider(_))
                    && values
                        .iter()
                        .any(|s| !matches!(s.as_str(), "claude-code" | "codex"))
                {
                    return Err(AppError::usage("unsupported provider in search filter"));
                }
            }
        }
        Ok(())
    }

    fn values(&self) -> &[String] {
        match self {
            Self::Node(v)
            | Self::Provider(v)
            | Self::Role(v)
            | Self::Conversation(v)
            | Self::Project(v)
            | Self::Repository(v) => v,
            _ => &[],
        }
    }

    /// Identifiers come only from this closed enum; literals are SQL quoted.
    /// COALESCE gives absent metadata the same Boolean semantics as Redis.
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

    pub(crate) fn redis(&self) -> String {
        match self {
            Self::All(v) => join_redis(v, " ", "@searchable:[0 +inf]"),
            Self::Any(v) => join_redis(v, " | ", "@searchable:[2 2]"),
            Self::Not(v) => format!("(-{})", v.redis()),
            Self::After(v) => format!("(@dated:[1 1] @created_at:[{} +inf])", v * 1000),
            Self::Before(v) => format!("(@dated:[1 1] @created_at:[-inf ({}])", v * 1000),
            _ => {
                let field = match self {
                    Self::Node(_) => "node_tag",
                    Self::Provider(_) => "provider_tag",
                    Self::Role(_) => "role_tag",
                    Self::Conversation(_) => "conversation",
                    Self::Project(_) => "project_tag",
                    Self::Repository(_) => "repository_tag",
                    _ => unreachable!(),
                };
                if self.values().is_empty() {
                    return "(@searchable:[2 2])".to_owned();
                }
                let values = self
                    .values()
                    .iter()
                    .map(|v| {
                        if matches!(self, Self::Provider(_)) {
                            if v == "codex" {
                                "x".to_owned()
                            } else {
                                "c".to_owned()
                            }
                        } else {
                            tag(v)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("|");
                format!("(@{field}:{{{values}}})")
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
fn join_redis(children: &[Filter], join: &str, empty: &str) -> String {
    if children.is_empty() {
        format!("({empty})")
    } else {
        format!(
            "({})",
            children
                .iter()
                .map(Filter::redis)
                .collect::<Vec<_>>()
                .join(join)
        )
    }
}
pub(crate) fn tag(value: &str) -> String {
    blake3::hash(value.as_bytes()).to_hex().to_string()
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
    fn expressions_validate_and_compile_without_interpolating_query_syntax() {
        let f: Filter = serde_json::from_str(r#"{"all":[{"any":[{"provider":["codex"]},{"role":["user"]}]},{"not":{"project":["x') OR TRUE --"]}},{"after":1}]}"#).unwrap();
        f.validate().unwrap();
        assert!(f.sql(Some("local")).contains("'x'') OR TRUE --'"));
        assert!(!f.redis().contains("OR TRUE"));
        assert_eq!(
            Filter::Node(vec!["local".into()]).sql(Some("local")),
            "COALESCE('local' IN ('local'), FALSE)"
        );
        assert_eq!(timestamp("2026-09-30").unwrap(), 1_790_726_400);
        assert!(timestamp("yesterday").is_err());
        assert!(Filter::Provider(vec!["invalid".into()]).validate().is_err());
        assert!(Filter::Project(vec!["nul\0".into()]).validate().is_err());
        let mut deep = Filter::All(vec![]);
        for _ in 0..10 {
            deep = Filter::Not(Box::new(deep));
        }
        assert!(deep.validate().is_err());
        assert!(serde_json::from_str::<Filter>(r#"{"project":["x"],"role":["user"]}"#).is_err());
    }
}
