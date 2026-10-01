//! Label-only continuation masking; never change source text or document vectors.
use super::discourse;

#[derive(Debug)]
enum Token {
    Word(String),
    Choice,
    Invalid,
}

fn selector(text: &str) -> bool {
    let text = text.trim();
    (text.len() == 1 && text.bytes().all(|b| b.is_ascii_uppercase())) || number(text)
}

fn number(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 2
        && text.bytes().all(|b| b.is_ascii_digit())
        && text.parse::<u8>().is_ok_and(|n| n > 0)
}

fn flush(word: &mut String, tokens: &mut Vec<Token>) {
    if word.is_empty() {
        return;
    }
    let word = std::mem::take(word);
    tokens.push(if number(&word) {
        Token::Choice
    } else {
        Token::Word(word)
    });
}

fn tokens(text: &str) -> Vec<Token> {
    let mut result = Vec::new();
    let mut word = String::new();
    let mut chars = text.char_indices().peekable();
    while let Some((index, character)) = chars.next() {
        if character == '('
            && let Some(close) = text[index + 1..].find(')')
            && close <= 4
            && selector(&text[index + 1..index + 1 + close])
        {
            flush(&mut word, &mut result);
            result.push(Token::Choice);
            let end = index + 1 + close;
            while chars.peek().is_some_and(|(i, _)| *i <= end) {
                chars.next();
            }
        } else if matches!(character, '(' | ')') {
            flush(&mut word, &mut result);
            result.push(Token::Invalid);
        } else if character.is_alphanumeric() {
            word.extend(character.to_lowercase());
        } else {
            flush(&mut word, &mut result);
        }
    }
    flush(&mut word, &mut result);
    result
}

fn words(tokens: &[Token]) -> Option<String> {
    tokens
        .iter()
        .map(|t| match t {
            Token::Word(word) => Some(word.as_str()),
            Token::Choice | Token::Invalid => None,
        })
        .collect::<Option<Vec<_>>>()
        .map(|words| words.join(" "))
}

fn choices(tokens: &[Token]) -> bool {
    let mut rest = tokens;
    loop {
        let Some((Token::Choice, tail)) = rest.split_first() else {
            return false;
        };
        rest = tail;
        if rest.is_empty() {
            return true;
        }
        if let Some(tail) = words(rest)
            && matches!(tail.as_str(), "then" | "overnight" | "overnight then")
        {
            return true;
        }
        match rest {
            [Token::Word(first), Token::Word(second), tail @ ..]
                if first == "followed" && second == "by" =>
            {
                rest = tail;
            }
            [Token::Word(join), tail @ ..] if matches!(join.as_str(), "and" | "then") => {
                rest = tail;
            }
            _ => return false,
        }
    }
}

/// Recognize complete short option acknowledgments, not an arbitrary request
/// containing a number. Unknown subjects, quoted code and malformed lists keep
/// their original labels. The indexing/discovery admission parser is unchanged.
fn option_only(text: &str) -> bool {
    if text.chars().count() > 256
        || text.contains(['`', '"', '-', '/', '+', '*', '=', '<', '>', '[', ']'])
    {
        return false;
    }
    let tokens = tokens(text);
    let Some(first) = tokens.iter().position(|t| matches!(t, Token::Choice)) else {
        return false;
    };
    if first == 0 {
        return words(&tokens[1..]).is_some_and(|tail| !tail.is_empty() && discourse::only(&tail));
    }
    let Some(prefix) = words(&tokens[..first]) else {
        return false;
    };
    let before = ["why don t we go ahead and do", "let s do", "we can do"]
        .iter()
        .find_map(|action| {
            prefix
                .strip_suffix(action)
                .filter(|before| before.is_empty() || before.ends_with(' '))
        });
    let Some(before) = before else { return false };
    let before = before.trim();
    let before = before
        .strip_prefix("also ")
        .or_else(|| before.strip_prefix("maybe "))
        .unwrap_or(before);
    let before = if matches!(before, "also" | "maybe") {
        ""
    } else {
        before
    };
    discourse::only(before) && choices(&tokens[first..])
}

fn continuation(text: &str) -> bool {
    if text.contains([
        '(', ')', '{', '}', '[', ']', '/', '\\', '_', '-', '+', '*', '=', '<', '>', '|', ':',
    ]) {
        return false;
    }
    let normalized = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ");
    !normalized.is_empty() && discourse::only(&normalized)
}

pub(super) fn mask(text: &str) -> String {
    // Keep quoted identifiers, code examples and literal acknowledgments intact.
    if text.contains(['`', '"']) {
        return text.to_owned();
    }
    if option_only(text) {
        return String::new();
    }
    let mut output = String::with_capacity(text.len());
    let mut offset = 0;
    for sentence in text.split_inclusive(['.', '!', '?', '\n', '\r']) {
        let identifier_tail = offset > 0
            && text[..offset].ends_with('.')
            && sentence.chars().next().is_some_and(char::is_alphanumeric);
        offset += sentence.len();
        // A period inside an identifier/path is not a sentence boundary.
        let identifier = sentence.ends_with('.')
            && text[offset..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '/' | '\\'));
        if !identifier && !identifier_tail && continuation(sentence) {
            // A separator prevents phrases from joining across a masked clause.
            output.push('.');
        } else {
            output.push_str(sentence);
        }
    }
    output
}

#[cfg(test)]
mod tests;
