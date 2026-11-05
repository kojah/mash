use crate::connectors::Role;

pub(crate) const ANCHOR_PREFIX: &str = "connector#";

pub(crate) fn parse_role(role: &str) -> Option<Role> {
    match role {
        "container" => Some(Role::Container),
        "segment" => Some(Role::Segment),
        "entry" => Some(Role::Entry),
        "document" => Some(Role::Document),
        _ => None,
    }
}

/// The role, source and object ID named by an anchor.
pub(crate) fn parse_anchor(anchor: &str) -> Option<(Role, &str, &str)> {
    let mut parts = anchor.strip_prefix(ANCHOR_PREFIX)?.splitn(3, '#');
    Some((parse_role(parts.next()?)?, parts.next()?, parts.next()?))
}
