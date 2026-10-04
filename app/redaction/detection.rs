//! Offline credential masking. This is a bounded policy, not proof of absence.
use regex::Regex;
use std::{ops::Range, sync::OnceLock};

const MARKER: &str = "[REDACTED_SECRET]";
const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_MATCHES: usize = 4096;

// Recognizable token formats follow Gitleaks' MIT-licensed rules; see
// research/redaction/README.md. Context rules also cover short/opaque values.
const PATTERNS: &[&str] = &[
    r"\b(?:gh[pousr]_[A-Za-z0-9]{36,255}|github_pat_[A-Za-z0-9_]{60,255})\b",
    r"\b(?:sk-(?:proj-|ant-api\d+-)?[A-Za-z0-9_-]{20,512}|AIza[A-Za-z0-9_-]{35}|(?:AKIA|ASIA)[A-Z0-9]{16}|xox[baprs]-[A-Za-z0-9-]{20,255})\b",
    r#"(?i)\b(?:authorization|proxy-authorization)['"]?\s*[:=]\s*['"]?(?:bearer|basic)\s+(?P<secret>[^\s'",;]+)"#,
    r"(?i)\b(?:postgres(?:ql)?|redis|rediss|mysql|mongodb(?:\+srv)?|https?|amqps?)://[^\s/@:]*:(?P<secret>[^\s/@]+)@",
    r#"(?i)\b(?:[a-z0-9]+[_-])*(?:password|passwd|pwd|secret|api[_-]?key|access[_-]?key|secret[_-]?key|client[_-]?secret|access[_-]?token|refresh[_-]?token|auth[_-]?token|token)['"]?\s*[:=]\s*"(?P<secret>(?:\\.|[^"\\\r\n])+)""#,
    r#"(?i)\b(?:[a-z0-9]+[_-])*(?:password|passwd|pwd|secret|api[_-]?key|access[_-]?key|secret[_-]?key|client[_-]?secret|access[_-]?token|refresh[_-]?token|auth[_-]?token|token)['"]?\s*[:=]\s*'(?P<secret>(?:\\.|[^'\\\r\n])+)'"#,
    r"(?i)\b(?:[a-z0-9]+[_-])*(?:password|passwd|pwd|secret|api[_-]?key|access[_-]?key|secret[_-]?key|client[_-]?secret|access[_-]?token|refresh[_-]?token|auth[_-]?token|token)\s*=\s*(?P<secret>[A-Za-z0-9_./+@%=-]+)",
    r"(?s)-----BEGIN (?:[A-Z0-9]+ )?PRIVATE KEY-----.*?(?:-----END (?:[A-Z0-9]+ )?PRIVATE KEY-----|\z)",
    r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b",
];

fn patterns() -> Option<&'static [Regex]> {
    static PATTERNS_COMPILED: OnceLock<Option<Vec<Regex>>> = OnceLock::new();
    PATTERNS_COMPILED
        .get_or_init(|| {
            PATTERNS
                .iter()
                .map(|p| Regex::new(p))
                .collect::<Result<_, _>>()
                .ok()
        })
        .as_deref()
}

fn placeholder(value: &str) -> bool {
    (value.starts_with("${") && value.ends_with('}'))
        || (value.starts_with('<') && value.ends_with('>'))
        || value.starts_with("std::")
        || value.starts_with("env::")
        || matches!(
            value,
            "[REDACTED_SECRET]"
                | "REDACTED"
                | "redacted"
                | "std"
                | "env"
                | "process"
                | "None"
                | "null"
                | "true"
                | "false"
        )
}

pub(crate) fn ranges(text: &str) -> Vec<Range<usize>> {
    let Some(patterns) = patterns().filter(|_| text.len() <= MAX_BYTES) else {
        return std::iter::once(0..text.len()).collect();
    };
    let mut ranges = Vec::new();
    for pattern in patterns {
        for capture in pattern.captures_iter(text) {
            let Some(secret) = capture.name("secret").or_else(|| capture.get(0)) else {
                continue;
            };
            if placeholder(secret.as_str()) {
                continue;
            }
            ranges.push(secret.range());
            if ranges.len() > MAX_MATCHES {
                return std::iter::once(0..text.len()).collect();
            }
        }
    }
    ranges.sort_unstable_by_key(|r| (r.start, r.end));
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

pub(crate) fn text(text: &str) -> String {
    replace(text, &ranges(text), false)
}

// Excerpts retain source character offsets. Mask the full source before slicing,
// so clipping a token cannot bypass detection. ASCII stars preserve byte/char
// offsets for ASCII secrets; Unicode secret characters retain their byte width.
pub(crate) fn projection(text: &str) -> String {
    replace(text, &ranges(text), true)
}

fn replace(text: &str, ranges: &[Range<usize>], preserve_offsets: bool) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    for range in ranges {
        output.push_str(&text[cursor..range.start]);
        if preserve_offsets {
            for character in text[range.clone()].chars() {
                output.push(match character.len_utf8() {
                    1 => '*',
                    2 => '×',
                    3 => '█',
                    _ => '🟥',
                });
            }
        } else {
            output.push_str(MARKER);
        }
        cursor = range.end;
    }
    output.push_str(&text[cursor..]);
    output
}
