//! Recognize complete generated envelopes while preserving unknown evidence.
mod environment;

const MAXIMUM_BYTES: usize = 65_536;

pub(super) fn project(text: &str) -> Option<String> {
    let text = text.trim();
    if text.len() > MAXIMUM_BYTES {
        return None;
    }
    let (header, body) = text.split_once('\n')?;
    if header == "# AGENTS.md instructions"
        || header
            .strip_prefix("# AGENTS.md instructions for ")
            .is_some_and(environment::absolute_path)
    {
        return instructions(body);
    }
    match header {
        "# Files mentioned by the user:" => attachments(body, false),
        "# Files pasted by the user:" => attachments(body, true),
        _ => None,
    }
}

fn instructions(body: &str) -> Option<String> {
    let body = body.trim_start().strip_prefix("<INSTRUCTIONS>")?;
    let (instructions, tail) = body.split_once("</INSTRUCTIONS>")?;
    if instructions.contains("<INSTRUCTIONS>")
        || tail.contains("<INSTRUCTIONS>")
        || tail.contains("</INSTRUCTIONS>")
    {
        return None;
    }
    let tail = tail.trim();
    // Only a verified generated context prefix is removed. Unknown context and
    // requests remain verbatim; no general-purpose XML stripping.
    Some(environment::strip(tail).unwrap_or(tail).trim().to_owned())
}

fn attachments(body: &str, pasted: bool) -> Option<String> {
    let (files, request) = body.split_once("\n## My request:")?;
    let request = if request.is_empty() {
        request
    } else {
        request.strip_prefix('\n')?
    };
    let mut titles = Vec::new();
    let mut count = 0;
    for line in files.lines().filter(|line| !line.trim().is_empty()) {
        count += 1;
        if count > 32 {
            return None;
        }
        let (name, path) = line.strip_prefix("## ")?.rsplit_once(": ")?;
        if name.is_empty() || !environment::absolute_path(path) {
            return None;
        }
        if pasted {
            let title = name.strip_prefix('"')?.strip_suffix('"')?;
            if title.is_empty() || title.contains('"') {
                return None;
            }
            // The quoted pasted-content description is meaningful source text,
            // unlike the generated attachment path and protocol heading.
            titles.push(title);
        }
    }
    if count == 0 {
        return None;
    }
    let mut output = titles.join("\n");
    if !output.is_empty() {
        output.push_str("\n\n");
    }
    let mut rest = request;
    while let Some(start) = rest.find("<image name=[Image #") {
        output.push_str(&rest[..start]);
        let image = &rest[start..];
        let open_end = image.find('>')?;
        let close = open_end + 1 + image[open_end + 1..].find("</image>")?;
        if !image[open_end + 1..close].trim().is_empty() {
            return None;
        }
        output.push('\n');
        rest = &image[close + "</image>".len()..];
    }
    output.push_str(rest);
    Some(output.trim().to_owned())
}

#[cfg(test)]
mod tests;
