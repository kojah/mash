//! Bounded recognizer of the observed Agentation page annotation schema.
//! On any unknown/malformed shape the caller keeps the entire original payload.
const MAXIMUM_BYTES: usize = 65_536;
const MAXIMUM_ANNOTATIONS: usize = 32;

struct Batch<'a> {
    page: &'a str,
    annotations: Vec<Annotation<'a>>,
}
struct Annotation<'a> {
    element: &'a str,
    feedback: &'a str,
}

pub(super) fn project(text: &str) -> Option<String> {
    let batch = parse(text)?;
    let mut output = String::new();
    for annotation in batch.annotations {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str(annotation.feedback);
        output.push('\n');
        output.push_str(annotation.element);
    }
    output.push_str("\n\nPage: ");
    output.push_str(batch.page);
    Some(output)
}

fn parse(text: &str) -> Option<Batch<'_>> {
    if text.len() > MAXIMUM_BYTES {
        return None;
    }
    let (heading, rest) = text.trim().split_once('\n')?;
    let page = heading.strip_prefix("## Page Feedback: ")?;
    if !page.starts_with('/') || page.len() > 2048 || page.chars().any(char::is_whitespace) {
        return None;
    }
    let (viewport, mut rest) = rest.split_once('\n')?;
    let (width, height) = viewport.strip_prefix("**Viewport:** ")?.split_once('×')?;
    if !dimension(width) || !dimension(height) {
        return None;
    }
    let mut annotations = Vec::new();
    while !rest.trim().is_empty() {
        if annotations.len() == MAXIMUM_ANNOTATIONS {
            return None;
        }
        let (heading, tail) = rest.trim_start_matches('\n').split_once('\n')?;
        let (number, element) = heading.strip_prefix("### ")?.split_once(". ")?;
        if number.parse::<usize>().ok()? != annotations.len() + 1
            || element.is_empty()
            || element.len() > 512
        {
            return None;
        }
        let (location, body) = tail.split_once('\n')?;
        if !selector(location.strip_prefix("**Location:** ")?) {
            return None;
        }
        let body = body.strip_prefix("**Feedback:** ")?;
        let boundary = body.find("\n\n### ").map_or(body.len(), |i| i + 2);
        let (feedback, tail) = body.split_at(boundary);
        let feedback = feedback.trim();
        if feedback.is_empty()
            || feedback.lines().any(|line| {
                line.starts_with("### ") || (line.starts_with("**") && line.contains(":**"))
            })
        {
            return None;
        }
        annotations.push(Annotation { element, feedback });
        rest = tail;
    }
    if annotations.is_empty() {
        return None;
    }
    Some(Batch { page, annotations })
}

fn dimension(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && text
            .parse::<u32>()
            .is_ok_and(|value| (1..=32_768).contains(&value))
}

fn selector(text: &str) -> bool {
    if text.is_empty() || text.len() > 4096 {
        return false;
    }
    text.split(" > ").all(|node| {
        let tag = node.split(['.', '#', ':']).next().unwrap_or_default();
        (node.starts_with(['.', '#'])
            || matches!(
                tag,
                "a" | "button"
                    | "code"
                    | "div"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "li"
                    | "p"
                    | "span"
                    | "strong"
                    | "table"
                    | "tbody"
                    | "td"
                    | "th"
                    | "thead"
                    | "tr"
            ))
            && node
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".#-_:()".contains(&b))
    })
}

#[cfg(test)]
mod tests;
