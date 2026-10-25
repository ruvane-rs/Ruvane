use super::*;
use crate::{Attribute, InternalUrl, Tag, element, fragment, link, text};

struct Budget {
    tokens: usize,
    work: usize,
}
impl Budget {
    fn work(&mut self, count: usize, line: usize) -> Result<(), ContentError> {
        self.work = self
            .work
            .checked_add(count)
            .filter(|n| *n <= MAX_MARKDOWN_WORK)
            .ok_or_else(|| error(ContentErrorKind::Complexity, line))?;
        Ok(())
    }
    fn token(&mut self, line: usize) -> Result<(), ContentError> {
        self.tokens += 1;
        if self.tokens > MAX_MARKDOWN_TOKENS {
            return Err(error(ContentErrorKind::Complexity, line));
        }
        Ok(())
    }
}
fn push(
    parts: &mut Vec<Html>,
    bytes: &mut usize,
    html: Html,
    line: usize,
) -> Result<(), ContentError> {
    *bytes += html.as_str().len();
    if *bytes > MAX_CONTENT_HTML_BYTES {
        return Err(error(ContentErrorKind::Limit, line));
    }
    parts.push(html);
    Ok(())
}

fn closing(
    input: &str,
    marker: &str,
    start: usize,
    budget: &mut Budget,
    line: usize,
) -> Result<Option<usize>, ContentError> {
    let bytes = input.as_bytes();
    let mut i = start;
    while i < bytes.len() {
        budget.work(1, line)?;
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if marker == "*" && bytes[i..].starts_with(b"**") {
            i += 2;
            continue;
        }
        if bytes[i..].starts_with(marker.as_bytes()) {
            return Ok(Some(i));
        }
        i += 1;
    }
    Ok(None)
}
fn inline(
    input: &str,
    depth: usize,
    links: bool,
    line: usize,
    budget: &mut Budget,
) -> Result<Html, ContentError> {
    if depth > MAX_MARKDOWN_DEPTH {
        return Err(error(ContentErrorKind::Complexity, line));
    }
    let mut parts = Vec::new();
    let mut bytes = 0;
    let mut i = 0;
    let mut line = line;
    while i < input.len() {
        budget.token(line)?;
        budget.work(1, line)?;
        let rest = &input[i..];
        if rest.starts_with("![") {
            return Err(error(ContentErrorKind::UnsupportedSyntax, line));
        }
        if let Some(escaped) = rest
            .strip_prefix('\\')
            .and_then(|s| s.chars().next())
            .filter(|c| c.is_ascii_punctuation())
        {
            push(&mut parts, &mut bytes, text(escaped.to_string()), line)?;
            i += 1 + escaped.len_utf8();
            continue;
        }
        let marker = if rest.starts_with("**") {
            Some(("**", Tag::Strong))
        } else if rest.starts_with('*') {
            Some(("*", Tag::Em))
        } else if rest.starts_with('`') {
            Some(("`", Tag::Code))
        } else {
            None
        };
        if let Some((marker, tag)) = marker {
            if let Some(end) = closing(input, marker, i + marker.len(), budget, line)? {
                if end > i + marker.len() {
                    let content = &input[i + marker.len()..end];
                    let body = if marker == "`" {
                        text(content)
                    } else {
                        inline(content, depth + 1, links, line, budget)?
                    };
                    push(
                        &mut parts,
                        &mut bytes,
                        element(tag).child(body).render(),
                        line,
                    )?;
                    let next = end + marker.len();
                    line += input[i..next].bytes().filter(|b| *b == b'\n').count();
                    i = next;
                    continue;
                }
            }
        }
        if rest.starts_with('[') {
            if let Some(end) = closing(input, "](", i + 1, budget, line)? {
                if let Some(close) = closing(input, ")", end + 2, budget, line)? {
                    if !links {
                        return Err(error(ContentErrorKind::UnsupportedSyntax, line));
                    }
                    let target = &input[end + 2..close];
                    let url = InternalUrl::new(target)
                        .map_err(|_| error(ContentErrorKind::InvalidLink, line))?;
                    let label = inline(&input[i + 1..end], depth + 1, false, line, budget)?;
                    push(&mut parts, &mut bytes, link(&url, label), line)?;
                    let next = close + 1;
                    line += input[i..next].bytes().filter(|b| *b == b'\n').count();
                    i = next;
                    continue;
                }
            }
        }
        let start = i;
        i += input[i..]
            .chars()
            .next()
            .expect("nonempty inline")
            .len_utf8();
        while i < input.len() && !b"*`[\\!".contains(&input.as_bytes()[i]) {
            budget.work(1, line)?;
            i += input[i..]
                .chars()
                .next()
                .expect("nonempty inline")
                .len_utf8();
        }
        push(&mut parts, &mut bytes, text(&input[start..i]), line)?;
        line += input[start..i].bytes().filter(|b| *b == b'\n').count();
    }
    Ok(fragment(parts))
}

fn heading(line: &str) -> Option<(Tag, &str)> {
    let count = line.bytes().take_while(|b| *b == b'#').count();
    let tag = match count {
        1 => Tag::H1,
        2 => Tag::H2,
        3 => Tag::H3,
        4 => Tag::H4,
        5 => Tag::H5,
        6 => Tag::H6,
        _ => return None,
    };
    line.get(count..)?.strip_prefix(' ').map(|body| (tag, body))
}
fn list_item(line: &str) -> Option<(Tag, &str)> {
    if let Some(body) = line.strip_prefix("- ") {
        return Some((Tag::Ul, body));
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=9).contains(&digits) {
        return line
            .get(digits..)?
            .strip_prefix(". ")
            .map(|body| (Tag::Ol, body));
    }
    None
}
fn block_start(line: &str) -> bool {
    line.is_empty()
        || line.starts_with("```")
        || heading(line).is_some()
        || list_item(line).is_some()
}

pub(super) fn render_lines(input: &[&str], offset: usize) -> Result<Html, ContentError> {
    let mut budget = Budget { tokens: 0, work: 0 };
    let mut parts = Vec::new();
    let mut bytes = 0;
    let mut i = 0;
    while i < input.len() {
        let line = offset + i + 1;
        budget.work(input[i].len(), line)?;
        if input[i].is_empty() {
            i += 1;
            continue;
        }
        budget.token(line)?;
        let html = if let Some(language) = input[i].strip_prefix("```") {
            if language.len() > 32
                || !language
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            {
                return Err(error(ContentErrorKind::UnsupportedSyntax, line));
            }
            i += 1;
            let mut code = String::new();
            while i < input.len() && input[i] != "```" {
                budget.work(input[i].len(), offset + i + 1)?;
                code.push_str(input[i]);
                code.push('\n');
                i += 1;
            }
            if i == input.len() {
                return Err(error(ContentErrorKind::UnclosedFence, line));
            }
            i += 1;
            let mut node = element(Tag::Code).child(code);
            if !language.is_empty() {
                node = node.attr(Attribute::Class, format!("language-{language}"));
            }
            element(Tag::Pre).child(node).render()
        } else if let Some((tag, body)) = heading(input[i]) {
            i += 1;
            element(tag)
                .child(inline(body, 0, true, line, &mut budget)?)
                .render()
        } else if let Some((tag, _)) = list_item(input[i]) {
            let mut list = element(tag);
            let mut size = 0;
            while i < input.len() {
                let Some((kind, body)) = list_item(input[i]) else {
                    break;
                };
                if kind != tag {
                    break;
                }
                budget.token(offset + i + 1)?;
                let item = element(Tag::Li)
                    .child(inline(body, 0, true, offset + i + 1, &mut budget)?)
                    .render();
                size += item.as_str().len();
                if size > MAX_CONTENT_HTML_BYTES {
                    return Err(error(ContentErrorKind::Limit, offset + i + 1));
                }
                list = list.child(item);
                i += 1;
            }
            list.render()
        } else {
            let mut paragraph = String::from(input[i]);
            i += 1;
            while i < input.len() && !block_start(input[i]) {
                paragraph.push('\n');
                paragraph.push_str(input[i]);
                i += 1;
            }
            element(Tag::P)
                .child(inline(&paragraph, 0, true, line, &mut budget)?)
                .render()
        };
        push(&mut parts, &mut bytes, html, line)?;
    }
    Ok(fragment(parts))
}

/// Renders a limited, bounded Markdown profile through typed HTML builders.
/// Supports ATX headings, paragraphs, flat lists, emphasis, code and query-free
/// internal links. Raw HTML is escaped. Images and nested links are rejected.
/// This is not a CommonMark implementation. Unmatched inline markers are text;
/// unclosed code fences are errors. See the content contract for exact limits.
pub fn markdown(source: &str) -> Result<Html, ContentError> {
    render_lines(&lines(source)?, 0)
}
