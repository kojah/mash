//! Copyable tool arguments derived from the source actually searched.
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lookup {
    pub view: View,
    pub list_attachments: Attachments,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Attachments {
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared: Option<String>,
}

pub fn attach(
    results: &mut super::Results,
    local_node: &str,
    shared_available: bool,
    shared_only: bool,
) {
    let lookup = |id: &str, node: Option<&str>| {
        if shared_only && shared_available && node.is_none() {
            return None;
        }
        let shared = node
            .filter(|node| shared_available && (shared_only || *node != local_node))
            .map(str::to_owned);
        Some(Lookup {
            view: View {
                id: id.to_owned(),
                shared: shared.clone(),
            },
            list_attachments: Attachments {
                message_id: id.to_owned(),
                shared,
            },
        })
    };
    match results {
        super::Results::Messages(hits) => {
            for hit in hits {
                hit.lookup = lookup(&hit.id, hit.node_id.as_deref());
            }
        }
        super::Results::Chats(threads) => {
            for hit in threads.iter_mut().flat_map(|thread| &mut thread.matches) {
                hit.lookup = lookup(&hit.message_id, hit.node_id.as_deref());
            }
        }
    }
}
