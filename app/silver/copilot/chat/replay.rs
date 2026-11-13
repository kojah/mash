//! Decode mutation values into their target Rust type, never a mutable JSON map.
use super::{
    super::Unreadable,
    invalid,
    models::{Prompt, Snapshot, Turn},
};
use crate::{AppError, silver::records};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::value::RawValue;
use std::{fs::File, io::BufReader, path::Path};

#[derive(Deserialize)]
#[serde(try_from = "u8")]
enum Kind {
    Initial,
    Set,
    Push,
    Delete,
}
impl TryFrom<u8> for Kind {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Initial),
            1 => Ok(Self::Set),
            2 => Ok(Self::Push),
            3 => Ok(Self::Delete),
            _ => Err("unsupported mutation kind"),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Segment {
    Field(String),
    Index(usize),
}

#[derive(Deserialize)]
struct Operation {
    kind: Kind,
    k: Option<Vec<Segment>>,
    // JSON's mutation value has no single schema. It is immediately decoded
    // into Snapshot, Turn, Part or a scalar selected by the typed path.
    v: Option<Box<RawValue>>,
    i: Option<usize>,
}

fn decode<T: DeserializeOwned>(value: Option<&RawValue>) -> Result<T, AppError> {
    records::decode(value.map_or(b"null".as_slice(), |v| v.get().as_bytes()))
}

fn required<T: DeserializeOwned>(value: Option<&RawValue>) -> Result<T, AppError> {
    decode(Some(
        value.ok_or_else(|| invalid("missing mutation value"))?,
    ))
}

fn splice<T: DeserializeOwned>(
    array: &mut Vec<T>,
    value: Option<&RawValue>,
    index: Option<usize>,
) -> Result<(), AppError> {
    let values: Vec<T> = if value.is_some() {
        required(value)?
    } else {
        Vec::new()
    };
    if let Some(index) = index {
        if index > array.len() {
            return Err(invalid("mutation array index out of range"));
        }
        array.truncate(index);
    }
    array.extend(values);
    Ok(())
}

fn set_request(
    request: &mut Turn,
    path: &[Segment],
    value: Option<&RawValue>,
    delete: bool,
) -> Result<(), AppError> {
    let Some(Segment::Field(field)) = path.first() else {
        return Err(invalid("invalid request mutation path"));
    };
    if path.len() == 2 && field == "message" && matches!(&path[1], Segment::Field(f) if f == "text")
    {
        request.message = Prompt::Text(required(value)?);
        return Ok(());
    }
    if path.len() != 1 {
        if matches!(
            field.as_str(),
            "message"
                | "requestId"
                | "response"
                | "timestamp"
                | "responseTimestamp"
                | "hiddenFromTranscript"
                | "requestHiddenFromTranscript"
                | "isHidden"
                | "isSystemInitiated"
        ) {
            return Err(invalid("unsupported mutation into a transcript field"));
        }
        return Ok(()); // draft input, attachments, tool state and UI metadata
    }
    match field.as_str() {
        "requestId" => request.request_id = required(value)?,
        "message" => request.message = required(value)?,
        "response" => request.response = decode(value)?,
        "agent" => request.agent = decode(value)?,
        "timestamp" => request.timestamp = decode(value)?,
        "responseTimestamp" => request.response_timestamp = decode(value)?,
        "modelState" => request.model_state = decode(value)?,
        "isHidden" => request.is_hidden = if delete { false } else { required(value)? },
        "hiddenFromTranscript" => {
            request.hidden_from_transcript = if delete { false } else { required(value)? }
        }
        "requestHiddenFromTranscript" => {
            request.prompt_hidden = if delete { false } else { required(value)? }
        }
        "isSystemInitiated" => {
            request.is_system_initiated = if delete { false } else { required(value)? }
        }
        _ => {}
    }
    Ok(())
}

fn apply(state: &mut Snapshot, operation: &Operation) -> Result<(), AppError> {
    let path = operation
        .k
        .as_deref()
        .ok_or_else(|| invalid("missing mutation path"))?;
    let value = operation.v.as_deref();
    let delete = matches!(operation.kind, Kind::Delete);
    if delete && value.is_some() {
        return Err(invalid("delete mutation has a value"));
    }
    let Some(Segment::Field(field)) = path.first() else {
        return if path.is_empty() && matches!(operation.kind, Kind::Set | Kind::Delete) {
            Ok(())
        } else {
            Err(invalid("invalid mutation path"))
        };
    };
    if field == "requests" {
        return apply_requests(state, path, operation, value, delete);
    }
    if matches!(operation.kind, Kind::Push) {
        return if matches!(
            field.as_str(),
            "sessionId"
                | "version"
                | "creationDate"
                | "customTitle"
                | "computedTitle"
                | "workingDirectory"
                | "repoData"
        ) {
            Err(invalid("invalid session array mutation"))
        } else {
            Ok(())
        };
    }
    if path.len() != 1 {
        // repoData is an atomic Adapt.value in the upstream schema.
        return if matches!(
            field.as_str(),
            "sessionId"
                | "version"
                | "creationDate"
                | "customTitle"
                | "computedTitle"
                | "workingDirectory"
                | "repoData"
        ) {
            Err(invalid("unsupported mutation into a session field"))
        } else {
            Ok(())
        };
    }
    match field.as_str() {
        "sessionId" => state.session_id = required(value)?,
        "version" => state.version = decode(value)?,
        "creationDate" => state.creation_date = required(value)?,
        "customTitle" => state.custom_title = decode(value)?,
        "computedTitle" => state.computed_title = decode(value)?,
        "workingDirectory" => state.working_directory = decode(value)?,
        "repoData" => state.repo_data = decode(value)?,
        _ => {}
    }
    Ok(())
}

pub(super) fn read(path: &Path, unreadable: &mut Unreadable) -> Result<Snapshot, AppError> {
    if !path.metadata().map_err(AppError::io)?.is_file() {
        return Err(invalid("log must be a regular file"));
    }
    let file = File::open(path).map_err(AppError::io)?;
    if !file.metadata().map_err(AppError::io)?.is_file() {
        return Err(invalid("log must be a regular file"));
    }
    let mut state = None;
    // Index-addressed mutations after an unreadable line cannot be replayed.
    for line in unreadable.lines(path, records::Lines::new(BufReader::new(file))) {
        let line = line?;
        let operation: Operation = records::decode(line.as_bytes())?;
        if matches!(operation.kind, Kind::Initial) {
            state = Some(required(operation.v.as_deref())?);
        } else {
            apply(
                state
                    .as_mut()
                    .ok_or_else(|| invalid("log missing initial snapshot"))?,
                &operation,
            )?;
        }
    }
    state.ok_or_else(|| invalid("empty operation log"))
}

fn apply_requests(
    state: &mut Snapshot,
    path: &[Segment],
    operation: &Operation,
    value: Option<&RawValue>,
    delete: bool,
) -> Result<(), AppError> {
    if path.len() == 1 {
        return match operation.kind {
            Kind::Push => splice(&mut state.requests, value, operation.i),
            Kind::Set => {
                state.requests = required(value)?;
                Ok(())
            }
            _ => Err(invalid("cannot delete requests")),
        };
    }
    let Some(Segment::Index(index)) = path.get(1) else {
        return Err(invalid("invalid request index"));
    };
    let request = state
        .requests
        .get_mut(*index)
        .ok_or_else(|| invalid("request index out of range"))?;
    if path.len() == 2 {
        if !matches!(operation.kind, Kind::Set) {
            return Err(invalid("invalid request replacement"));
        }
        *request = required(value)?;
        return Ok(());
    }
    if matches!(&path[2], Segment::Field(f) if f == "response") {
        if path.len() == 3 && matches!(operation.kind, Kind::Push) {
            return splice(
                request.response.get_or_insert_default().parts_mut(),
                value,
                operation.i,
            );
        }
        if path.len() == 4 && matches!(operation.kind, Kind::Set) {
            let Some(Segment::Index(index)) = path.get(3) else {
                return Err(invalid("invalid response index"));
            };
            let part = request
                .response
                .as_mut()
                .and_then(|r| r.parts_mut().get_mut(*index))
                .ok_or_else(|| invalid("response index out of range"))?;
            *part = required(value)?;
            return Ok(());
        }
    }
    if matches!(operation.kind, Kind::Push) {
        if matches!(&path[2], Segment::Field(f) if f == "message" || f == "response") {
            return Err(invalid("invalid transcript array mutation"));
        }
        return Ok(());
    }
    set_request(request, &path[2..], value, delete)
}
