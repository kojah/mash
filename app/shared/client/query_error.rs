//! Preserve only recognized, content-free query guidance from the shared API.
use crate::{AppError, ErrorKind};

const MAX_BYTES: usize = 16 * 1024;

pub(super) async fn read(mut response: reqwest::Response) -> Option<AppError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if chunk.len() > MAX_BYTES - bytes.len() {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    let error: AppError = serde_json::from_slice(&bytes).ok()?;
    if error.error.kind != ErrorKind::Usage {
        return None;
    }
    // Reconstruct locally: never forward arbitrary response text, database
    // details, actions or retry policy from a server or intervening proxy.
    let message = match error.error.message.as_str() {
        crate::shared::EXACT_SEARCH_BUDGET_ERROR => crate::shared::EXACT_SEARCH_BUDGET_ERROR,
        // Installed servers may still use the previous, content-free wording.
        "exact query exceeded the shared search budget; narrow the query or metadata filters" => {
            crate::shared::EXACT_SEARCH_BUDGET_ERROR
        }
        "shared exact candidate budget reached (10000); narrow the query or metadata filters" => {
            "shared exact candidate budget reached (10000); narrow the query or metadata filters"
        }
        "query exceeded its time budget" => {
            "shared query exceeded its time budget; narrow the query or choose a smaller node"
        }
        "node is not in this group" => "shared query named a node outside this group",
        "query may only read; ATTACH and pragma are not allowed" => {
            "shared query may only read; ATTACH and pragma are not allowed"
        }
        "this target cannot group by role or conversation" => {
            "shared query cannot group this target by role or conversation"
        }
        "lexical matching vocabulary budget exceeded; narrow filters or use match_mode=strict" => {
            "lexical matching vocabulary budget exceeded; narrow filters or use match_mode=strict"
        }
        _ => return None,
    };
    Some(AppError::usage(message))
}
