//! Compile bounded disjunctive normal form into native metadata constraints.
//! Excessive expansion uses a canonical eligible-key constraint, never a
//! postfilter. NULL branches preserve Mash's COALESCE(..., FALSE) semantics.
use crate::search_filter::Filter;

const MAX_BRANCHES: usize = 64;
type Branches = Vec<Vec<String>>;

pub(super) enum Plan {
    Direct(Branches),
    Empty,
    EligibleKeys,
}

pub(super) fn plan(filter: &Filter, node: &str) -> Plan {
    match compile(filter, node, false) {
        Some(branches) if branches.is_empty() => Plan::Empty,
        Some(branches) => Plan::Direct(branches),
        None => Plan::EligibleKeys,
    }
}

fn compile(filter: &Filter, node: &str, negated: bool) -> Option<Branches> {
    match filter {
        Filter::Not(child) => compile(child, node, !negated),
        Filter::All(children) => combine(children, node, !negated, negated),
        Filter::Any(children) => combine(children, node, negated, negated),
        Filter::Node(values) => Some(if values.iter().any(|value| value == node) == negated {
            Vec::new()
        } else {
            vec![Vec::new()]
        }),
        Filter::After(value) | Filter::Before(value) => {
            let op = match (matches!(filter, Filter::After(_)), negated) {
                (true, false) | (false, true) => ">=",
                (false, false) | (true, true) => "<",
            };
            let mut branches = vec![vec![format!("v.created_at {op} {}", value * 1000)]];
            if negated {
                branches.push(vec!["v.created_at IS NULL".into()]);
            }
            Some(branches)
        }
        _ => values(filter, negated),
    }
}

fn combine(children: &[Filter], node: &str, conjunction: bool, negated: bool) -> Option<Branches> {
    let mut result = if conjunction {
        vec![Vec::new()]
    } else {
        Vec::new()
    };
    for child in children {
        let branches = compile(child, node, negated)?;
        if conjunction {
            if result.len().saturating_mul(branches.len()) > MAX_BRANCHES {
                return None;
            }
            result = result
                .iter()
                .flat_map(|left| {
                    branches.iter().map(move |right| {
                        let mut branch = left.clone();
                        branch.extend(right.iter().cloned());
                        branch.sort();
                        branch.dedup();
                        branch
                    })
                })
                .collect();
        } else {
            result.extend(branches);
        }
        result.sort();
        result.dedup();
        if result.len() > MAX_BRANCHES {
            return None;
        }
    }
    if result.iter().any(Vec::is_empty) {
        result = vec![Vec::new()];
    }
    Some(result)
}

fn values(filter: &Filter, negated: bool) -> Option<Branches> {
    let column = match filter {
        Filter::Provider(_) => "provider",
        Filter::Role(_) => "role",
        Filter::Chat(_) => "chat",
        Filter::Project(_) => "project",
        Filter::Repository(_) => "repository",
        Filter::Source(_) => "source",
        Filter::Kind(_) => "kind",
        _ => unreachable!(),
    };
    let mut values = filter.values();
    values.sort_unstable();
    values.dedup();
    if values.is_empty() {
        return Some(if negated {
            vec![Vec::new()]
        } else {
            Vec::new()
        });
    }
    if !negated {
        return Some(vec![vec![format!(
            "v.{column} IN ({})",
            values.into_iter().map(quote).collect::<Vec<_>>().join(",")
        )]]);
    }
    // NOT IN must include absent metadata, unlike SQL's three-valued NOT IN.
    let mut branches = vec![
        vec![format!("v.{column} IS NULL")],
        vec![format!("v.{column} < {}", quote(values[0]))],
        vec![format!("v.{column} > {}", quote(values[values.len() - 1]))],
    ];
    for interval in values.windows(2) {
        branches.push(vec![
            format!("v.{column} > {}", quote(interval[0])),
            format!("v.{column} < {}", quote(interval[1])),
        ]);
    }
    if branches.len() > MAX_BRANCHES {
        None
    } else {
        Some(branches)
    }
}

pub(super) fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
