//! Small recognizer for the observed context schema; unknown variants survive.

pub(super) fn absolute_path(path: &str) -> bool {
    path.starts_with('/') && !path.contains(['<', '>', '\n', '\r']) && path.len() <= 4096
}

fn field<'a>(text: &'a str, name: &str) -> Option<(&'a str, &'a str)> {
    let body = text.trim_start().strip_prefix(&format!("<{name}>"))?;
    let (value, rest) = body.split_once(&format!("</{name}>"))?;
    if value.is_empty() || value.contains(['<', '>', '\n', '\r']) {
        return None;
    }
    Some((value, rest))
}

pub(super) fn strip(text: &str) -> Option<&str> {
    let mut rest = text.strip_prefix("<environment_context>")?;
    if rest.trim_start().starts_with("<cwd>") {
        let (cwd, tail) = field(rest, "cwd")?;
        if !absolute_path(cwd) {
            return None;
        }
        rest = tail;
    }
    if rest.trim_start().starts_with("<shell>") {
        let (shell, tail) = field(rest, "shell")?;
        if !matches!(
            shell,
            "bash" | "zsh" | "sh" | "fish" | "pwsh" | "powershell" | "cmd"
        ) {
            return None;
        }
        rest = tail;
    }
    let (date, tail) = field(rest, "current_date")?;
    if date.len() != 10
        || date.bytes().enumerate().any(|(i, b)| {
            if i == 4 || i == 7 {
                b != b'-'
            } else {
                !b.is_ascii_digit()
            }
        })
    {
        return None;
    }
    let (timezone, tail) = field(tail, "timezone")?;
    if !timezone
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b))
    {
        return None;
    }
    let mut rest = tail
        .trim_start()
        .strip_prefix("<filesystem><workspace_roots>")?;
    let mut roots = 0;
    while rest.starts_with("<root>") {
        let (path, tail) = field(rest, "root")?;
        roots += 1;
        if roots > 32 || !absolute_path(path) {
            return None;
        }
        rest = tail;
    }
    if roots == 0 {
        return None;
    }
    rest = rest.strip_prefix("</workspace_roots><permission_profile type=\"disabled\"><file_system type=\"unrestricted\" /></permission_profile></filesystem>")?;
    let tail = rest.trim_start().strip_prefix("</environment_context>")?;
    if tail.contains("<environment_context>") || tail.contains("</environment_context>") {
        return None;
    }
    Some(tail)
}
