//! View query models: selection and boolean predicates over silver metadata.
use crate::provider::Provider;
use rmcp::schemars;
use serde::{Deserialize, Serialize};

use crate::AppError;

/// The order of results that qualify as evidence.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    clap::ValueEnum,
    schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Sort {
    /// Most relevant first, with newer messages favoured among comparable ones.
    #[default]
    Relevance,
    /// Newest first within the returned page; a chat sorts by its latest matching message.
    Recent,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Selection {
    /// flexible (default) enables prefix and typo matching; strict preserves literals.
    pub(crate) match_mode: crate::views::matching::Mode,
    /// relevance (default) or recent: newest first within each page of relevant results.
    pub(crate) sort: Sort,
    /// Provider alternatives; omitted or empty selects all providers.
    pub(crate) provider: Vec<Provider>,
    pub(crate) days: Option<u32>,
    pub(crate) node: Vec<String>,
    pub(crate) role: Vec<crate::role::Role>,
    /// Canonical chat IDs from search or `list_chats`; omitted or empty selects all chats.
    #[serde(rename = "chat")]
    pub(crate) conversation: Vec<String>,
    /// Exact recorded working directory; repeat for alternatives.
    pub(crate) project: Vec<String>,
    /// Exact Git origin URL; repeat for alternatives.
    pub(crate) repository: Vec<String>,
    /// Connector source IDs; native provider history never matches.
    pub(crate) source: Vec<String>,
    /// Object kind labels, such as `codex:message` or `slack:message`.
    pub(crate) kind: Vec<String>,
    /// Inclusive Unix timestamp in seconds.
    pub(crate) after: Option<i64>,
    /// Exclusive Unix timestamp in seconds.
    pub(crate) before: Option<i64>,
    /// Boolean expression using all/any/not and metadata predicates.
    pub(crate) filter: Option<Filter>,
}

#[derive(Clone)]
pub(crate) struct Scope {
    pub(crate) match_mode: crate::views::matching::Mode,
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
    Chat,
    Project,
    Repository,
    Source,
    Kind,
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
            (!self.conversation.is_empty(), Field::Chat),
            (!self.project.is_empty(), Field::Project),
            (!self.repository.is_empty(), Field::Repository),
            (!self.source.is_empty(), Field::Source),
            (!self.kind.is_empty(), Field::Kind),
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
            (self.conversation, Filter::Chat),
            (self.project, Filter::Project),
            (self.repository, Filter::Repository),
            (self.source, Filter::Source),
            (self.kind, Filter::Kind),
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
            match_mode: self.match_mode,
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
    Chat(Vec<String>),
    Project(Vec<String>),
    Repository(Vec<String>),
    /// Connector source IDs; native provider history never matches.
    Source(Vec<String>),
    /// Object kind labels; native messages are `<provider>:message`.
    Kind(Vec<String>),
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
            Self::Chat(_) => Field::Chat,
            Self::Project(_) => Field::Project,
            Self::Repository(_) => Field::Repository,
            Self::Source(_) => Field::Source,
            Self::Kind(_) => Field::Kind,
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

    pub(crate) fn values(&self) -> Vec<&str> {
        match self {
            Self::Node(v)
            | Self::Chat(v)
            | Self::Project(v)
            | Self::Repository(v)
            | Self::Source(v)
            | Self::Kind(v) => v.iter().map(String::as_str).collect(),
            Self::Provider(v) => v.iter().map(|p| p.as_str()).collect(),
            Self::Role(v) => v.iter().map(crate::role::Role::as_str).collect(),
            _ => Vec::new(),
        }
    }

    /// Native message predicates over the `m` and `c` aliases.
    pub(crate) fn sql(&self, local_node: Option<&str>) -> String {
        self.compile(&Columns::NATIVE, local_node)
    }

    /// Connector unit predicates over the `u` alias. Connector records carry no
    /// provider, role, project or repository, so those predicates never match.
    pub(crate) fn connector_sql(&self, local_node: Option<&str>) -> String {
        self.compile(&Columns::CONNECTOR, local_node)
    }

    /// A condition every matching native message satisfies, over the chat
    /// `column` that `message_fts` stores beside each indexed message. Filtered
    /// lexical scans check it before joining silver objects, so a chat-scoped
    /// search does not look up every indexed message. It never replaces `sql`.
    pub(crate) fn fts_chat_sql(&self, column: &str) -> String {
        match self.implied_chats() {
            None => "TRUE".to_owned(),
            Some(chats) if chats.is_empty() => "FALSE".to_owned(),
            Some(chats) => format!(
                "{column} IN ({})",
                chats.into_iter().map(quote).collect::<Vec<_>>().join(",")
            ),
        }
    }

    /// Chats that contain every match, or `None` when the filter allows any chat.
    fn implied_chats(&self) -> Option<std::collections::BTreeSet<&str>> {
        match self {
            Self::Chat(values) => Some(values.iter().map(String::as_str).collect()),
            Self::All(children) => children
                .iter()
                .filter_map(Self::implied_chats)
                .reduce(|left, right| left.intersection(&right).copied().collect()),
            Self::Any(children) => {
                children
                    .iter()
                    .try_fold(std::collections::BTreeSet::new(), |mut chats, child| {
                        chats.extend(child.implied_chats()?);
                        Some(chats)
                    })
            }
            _ => None,
        }
    }

    /// Identifiers come only from these fixed column maps; literals are SQL quoted.
    /// COALESCE preserves the filter tree's Boolean semantics for absent metadata.
    fn compile(&self, columns: &Columns, local_node: Option<&str>) -> String {
        match self {
            Self::All(v) => join_sql(v, " AND ", "TRUE", columns, local_node),
            Self::Any(v) => join_sql(v, " OR ", "FALSE", columns, local_node),
            Self::Not(v) => format!("(NOT {})", v.compile(columns, local_node)),
            Self::After(v) => format!("COALESCE({} >= {}, FALSE)", columns.created_at, v * 1000),
            Self::Before(v) => format!("COALESCE({} < {}, FALSE)", columns.created_at, v * 1000),
            _ => {
                let column = match self {
                    Self::Node(_) => local_node.map_or_else(|| columns.node.to_owned(), quote),
                    Self::Provider(_) => columns.provider.to_owned(),
                    Self::Role(_) => columns.role.to_owned(),
                    Self::Chat(_) => columns.chat.to_owned(),
                    Self::Project(_) => columns.project.to_owned(),
                    Self::Repository(_) => columns.repository.to_owned(),
                    Self::Source(_) => columns.source.to_owned(),
                    Self::Kind(_) => columns.kind.to_owned(),
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

/// SQL expressions for each predicate field; `NULL` where a row has no such field.
struct Columns {
    node: &'static str,
    provider: &'static str,
    role: &'static str,
    chat: &'static str,
    project: &'static str,
    repository: &'static str,
    source: &'static str,
    kind: &'static str,
    created_at: &'static str,
}

impl Columns {
    const NATIVE: Self = Self {
        node: "m.node_id",
        provider: "c.source",
        role: "m.role",
        chat: "m.conversation_id",
        project: "c.project",
        repository: "c.repository",
        source: "NULL",
        kind: "m.kind",
        created_at: "m.created_at",
    };
    const CONNECTOR: Self = Self {
        node: "NULL",
        provider: "NULL",
        role: "NULL",
        chat: "u.group_anchor",
        project: "NULL",
        repository: "NULL",
        source: "u.source_id",
        kind: "u.label",
        created_at: "u.created_at",
    };
}

fn join_sql(
    children: &[Filter],
    join: &str,
    empty: &str,
    columns: &Columns,
    node: Option<&str>,
) -> String {
    if children.is_empty() {
        empty.to_owned()
    } else {
        format!(
            "({})",
            children
                .iter()
                .map(|f| f.compile(columns, node))
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

    #[test]
    fn fts_chat_prefilter_is_implied_by_the_filter_tree() {
        let chats = |json: &str| {
            serde_json::from_str::<Filter>(json)
                .unwrap()
                .fts_chat_sql("f.conversation_id")
        };
        assert_eq!(
            chats(r#"{"all":[{"role":["user"]},{"chat":["b","a'x"]}]}"#),
            "f.conversation_id IN ('a''x','b')"
        );
        assert_eq!(
            chats(r#"{"all":[{"chat":["a","b"]},{"all":[{"chat":["b","c"]}]}]}"#),
            "f.conversation_id IN ('b')"
        );
        assert_eq!(
            chats(r#"{"any":[{"chat":["a"]},{"all":[{"chat":["b"]},{"role":["user"]}]}]}"#),
            "f.conversation_id IN ('a','b')"
        );
        assert_eq!(chats(r#"{"all":[{"chat":["a"]},{"chat":["b"]}]}"#), "FALSE");
        assert_eq!(chats(r#"{"any":[]}"#), "FALSE");
        for open in [
            r#"{"all":[]}"#,
            r#"{"role":["user"]}"#,
            r#"{"any":[{"chat":["a"]},{"role":["user"]}]}"#,
            r#"{"not":{"chat":["a"]}}"#,
        ] {
            assert_eq!(chats(open), "TRUE", "{open}");
        }
    }
}
