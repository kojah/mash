//! Structured tool activity of a native entry, stored as its silver `metadata`.
//! Names and call identities are typed; tool inputs stay opaque provider JSON,
//! kept verbatim.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::AppError;

/// Bounds external activity records: published batches and imported archives.
const MAX_ITEMS: usize = 1024;
const MAX_LABEL_BYTES: usize = 4096;

/// The tool calls a native entry makes and the results it reports.
/// One provider message can carry several of each.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Activity {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) tool_results: Vec<ToolResult>,
}

/// One requested tool invocation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolCall {
    /// The provider's tool name; absent only where the provider omits it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    /// Links results to this call within the chat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) call_id: Option<String>,
    /// The provider's arguments, verbatim; a JSON string when the provider
    /// records encoded or free-form arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) input: Option<Value>,
}

/// One reported tool result. Its output stays in the entry's content.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) call_id: Option<String>,
    /// Present only where the provider records an error flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) is_error: Option<bool>,
}

impl Activity {
    pub(crate) fn is_empty(&self) -> bool {
        self.tool_calls.is_empty() && self.tool_results.is_empty()
    }

    pub(crate) fn call(name: Option<&str>, call_id: Option<&str>, input: Option<&Value>) -> Self {
        Self {
            tool_calls: vec![ToolCall {
                name: name.map(str::to_owned),
                call_id: call_id.map(str::to_owned),
                input: input.cloned(),
            }],
            tool_results: Vec::new(),
        }
    }

    pub(crate) fn result(call_id: Option<&str>, is_error: Option<bool>) -> Self {
        Self {
            tool_calls: Vec::new(),
            tool_results: vec![ToolResult {
                call_id: call_id.map(str::to_owned),
                is_error,
            }],
        }
    }

    pub(crate) fn extend(&mut self, other: Self) {
        self.tool_calls.extend(other.tool_calls);
        self.tool_results.extend(other.tool_results);
    }

    /// The silver `metadata` encoding: `None` when the entry has no activity.
    pub(crate) fn encode(&self) -> Result<Option<String>, AppError> {
        if self.is_empty() {
            return Ok(None);
        }
        serde_json::to_string(self)
            .map(Some)
            .map_err(|_| AppError::internal("serialize tool activity"))
    }

    pub(crate) fn decode(value: Option<&str>) -> Result<Self, AppError> {
        value.map_or_else(
            || Ok(Self::default()),
            |json| {
                serde_json::from_str(json)
                    .map_err(|_| AppError::database_data("invalid tool activity metadata"))
            },
        )
    }

    /// Approximate retained heap bytes, for bounded message staging.
    pub(crate) fn retained_size(&self) -> usize {
        let label = |value: &Option<String>| value.as_ref().map_or(0, String::capacity);
        self.tool_calls
            .iter()
            .map(|call| {
                std::mem::size_of::<ToolCall>()
                    + label(&call.name)
                    + label(&call.call_id)
                    + call.input.as_ref().map_or(0, value_size)
            })
            .chain(
                self.tool_results
                    .iter()
                    .map(|result| std::mem::size_of::<ToolResult>() + label(&result.call_id)),
            )
            .sum()
    }

    /// External records name tools with bounded single-line labels.
    pub(crate) fn validate(&self) -> Result<(), AppError> {
        let label = |value: &Option<String>| {
            value.as_ref().is_none_or(|value| {
                !value.is_empty()
                    && value.len() <= MAX_LABEL_BYTES
                    && !value.contains(char::is_control)
            })
        };
        if self.tool_calls.len() > MAX_ITEMS
            || self.tool_results.len() > MAX_ITEMS
            || !self
                .tool_calls
                .iter()
                .all(|call| label(&call.name) && label(&call.call_id))
            || !self
                .tool_results
                .iter()
                .all(|result| label(&result.call_id))
        {
            return Err(AppError::usage("invalid tool activity"));
        }
        Ok(())
    }

    /// Mask recognizable credentials in every label and string leaf of the
    /// opaque inputs, keeping the inputs valid JSON.
    pub(crate) fn redact(&mut self) {
        let mask = |value: &mut Option<String>| {
            if let Some(value) = value {
                *value = crate::redaction::text(value);
            }
        };
        for call in &mut self.tool_calls {
            mask(&mut call.name);
            mask(&mut call.call_id);
            if let Some(input) = &mut call.input {
                redact_value(input);
            }
        }
        for result in &mut self.tool_results {
            mask(&mut result.call_id);
        }
    }
}

fn value_size(value: &Value) -> usize {
    std::mem::size_of::<Value>()
        + match value {
            Value::String(text) => text.capacity(),
            Value::Array(items) => items.iter().map(value_size).sum(),
            Value::Object(map) => map
                .iter()
                .map(|(key, value)| key.capacity() + value_size(value))
                .sum(),
            Value::Null | Value::Bool(_) | Value::Number(_) => 0,
        }
}

fn redact_value(value: &mut Value) {
    match value {
        Value::String(text) => *text = crate::redaction::text(text),
        Value::Array(items) => items.iter_mut().for_each(redact_value),
        Value::Object(map) => map.values_mut().for_each(redact_value),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests;
