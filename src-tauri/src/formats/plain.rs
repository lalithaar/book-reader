use std::path::Path;

use super::{Built, Parsed};

/// Roughly a screenful times ten: big enough that chapters do not feel
/// chopped up, small enough that parsing and pagination stay instant.
const TARGET_CHARS: usize = 12_000;

pub fn import_txt(path: &Path) -> Result<Parsed, String> {
    let text = super::read_text(path)?;
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");

    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in normalized.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(trimmed);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }

    // A "paragraph" can be an entire file (no blank lines anywhere), so cut the
    // pathological ones at word boundaries before chunking.
    let mut split: Vec<String> = Vec::new();
    for paragraph in paragraphs {
        let mut rest = paragraph;
        while rest.chars().count() > TARGET_CHARS {
            let cut = rest
                .char_indices()
                .nth(TARGET_CHARS)
                .map(|(i, _)| i)
                .unwrap_or(rest.len());
            let boundary = rest[..cut].rfind(' ').unwrap_or(cut);
            let head = rest[..boundary].trim_end().to_string();
            if !head.is_empty() {
                split.push(head);
            }
            rest = rest[boundary..].trim_start().to_string();
        }
        if !rest.is_empty() {
            split.push(rest);
        }
    }
    let paragraphs = split;

    let book_title = paragraphs
        .first()
        .filter(|p| p.chars().count() <= 90)
        .cloned()
        .unwrap_or_else(|| super::file_stem(path));

    let mut chunks: Vec<Vec<String>> = Vec::new();
    let mut chunk: Vec<String> = Vec::new();
    let mut size = 0usize;
    for paragraph in paragraphs {
        size += paragraph.chars().count();
        chunk.push(paragraph);
        if size >= TARGET_CHARS {
            chunks.push(std::mem::take(&mut chunk));
            size = 0;
        }
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }

    let single = chunks.len() <= 1;
    let chapters = chunks
        .into_iter()
        .enumerate()
        .map(|(i, paragraphs)| {
            let mut out = String::new();
            for paragraph in paragraphs {
                out.push_str("<p>");
                out.push_str(&html_escape::encode_text(&paragraph));
                out.push_str("</p>");
            }
            let title = if single {
                book_title.clone()
            } else {
                format!("Part {}", i + 1)
            };
            Built::new(title, out, i)
        })
        .collect();

    Ok(Parsed {
        title: book_title,
        author: "Unknown".into(),
        language: String::new(),
        cover: None,
        chapters,
        toc: Vec::new(),
    })
}

pub fn import_md(path: &Path) -> Result<Parsed, String> {
    let text = super::read_text(path)?;
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");

    let mut chapters: Vec<Built> = Vec::new();
    let mut title = String::new();
    let mut current_title = String::new();
    let mut body: Vec<&str> = Vec::new();

    let flush = |body: &mut Vec<&str>, current_title: &str, chapters: &mut Vec<Built>| {
        if body.is_empty() {
            return;
        }
        let html = markdown(body);
        let key = chapters.len();
        chapters.push(Built::new(current_title.to_string(), html, key));
        body.clear();
    };

    for line in normalized.lines() {
        let is_heading = line.starts_with("# ") || line.starts_with("## ");
        if is_heading && !body.iter().all(|l| l.trim().is_empty()) {
            flush(&mut body, &current_title, &mut chapters);
        }
        if is_heading {
            let level = if line.starts_with("## ") { 2 } else { 1 };
            let heading = line.trim_start_matches('#').trim().to_string();
            if current_title.is_empty() && level == 1 {
                title = heading.clone();
            }
            current_title = heading;
        }
        body.push(line);
    }
    flush(&mut body, &current_title, &mut chapters);

    if chapters.is_empty() {
        return Err("this Markdown file has no text".into());
    }

    Ok(Parsed {
        title: if title.is_empty() { super::file_stem(path) } else { title },
        author: "Unknown".into(),
        language: String::new(),
        cover: None,
        chapters,
        toc: Vec::new(),
    })
}

#[allow(unused_assignments)]
fn markdown(lines: &[&str]) -> String {
    let mut out = String::new();
    let mut paragraph = String::new();
    let mut list: Option<&str> = None;
    let mut quote = false;
    let mut code = false;
    let mut code_buf = String::new();

    macro_rules! close_blocks {
        () => {
            if !paragraph.is_empty() {
                out.push_str("<p>");
                out.push_str(&inline(&paragraph));
                out.push_str("</p>");
                paragraph.clear();
            }
            if let Some(tag) = list.take() {
                out.push_str(if tag == "ul" { "</ul>" } else { "</ol>" });
            }
            if quote {
                out.push_str("</blockquote>");
                quote = false;
            }
        };
    }

    for line in lines {
        let trimmed = line.trim_end();

        if trimmed.trim_start().starts_with("```") {
            if code {
                out.push_str("<pre><code>");
                out.push_str(&html_escape::encode_text(&code_buf));
                out.push_str("</code></pre>");
                code_buf.clear();
                code = false;
            } else {
                close_blocks!();
                code = true;
            }
            continue;
        }
        if code {
            code_buf.push_str(trimmed);
            code_buf.push('\n');
            continue;
        }

        let text = trimmed.trim_start();
        if text.is_empty() {
            close_blocks!();
            continue;
        }

        if let Some(level) = heading_level(text) {
            close_blocks!();
            let content = text.trim_start_matches('#').trim();
            out.push_str(&format!(
                "<h{0}>{1}</h{0}>",
                level,
                inline(content)
            ));
            continue;
        }

        if text.starts_with("---") || text.starts_with("***") {
            close_blocks!();
            out.push_str("<hr>");
            continue;
        }

        if let Some(rest) = text.strip_prefix("> ") {
            if !paragraph.is_empty() {
                out.push_str("<p>");
                out.push_str(&inline(&paragraph));
                out.push_str("</p>");
                paragraph.clear();
            }
            if !quote {
                out.push_str("<blockquote>");
                quote = true;
            }
            out.push_str("<p>");
            out.push_str(&inline(rest));
            out.push_str("</p>");
            continue;
        }

        if let Some(rest) = text.strip_prefix("- ").or_else(|| text.strip_prefix("* ")) {
            if !paragraph.is_empty() {
                out.push_str("<p>");
                out.push_str(&inline(&paragraph));
                out.push_str("</p>");
                paragraph.clear();
            }
            if list != Some("ul") {
                if let Some(tag) = list.take() {
                    out.push_str(if tag == "ul" { "</ul>" } else { "</ol>" });
                }
                out.push_str("<ul>");
                list = Some("ul");
            }
            out.push_str("<li>");
            out.push_str(&inline(rest));
            out.push_str("</li>");
            continue;
        }

        if let Some(rest) = ordered_item(text) {
            if !paragraph.is_empty() {
                out.push_str("<p>");
                out.push_str(&inline(&paragraph));
                out.push_str("</p>");
                paragraph.clear();
            }
            if list != Some("ol") {
                if let Some(tag) = list.take() {
                    out.push_str(if tag == "ul" { "</ul>" } else { "</ol>" });
                }
                out.push_str("<ol>");
                list = Some("ol");
            }
            out.push_str("<li>");
            out.push_str(&inline(rest));
            out.push_str("</li>");
            continue;
        }

        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(text);
    }

    close_blocks!();
    out
}

fn heading_level(text: &str) -> Option<usize> {
    let hashes = text.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) {
        let rest = text[hashes..].trim_start();
        if !rest.is_empty() && text[hashes..].starts_with(' ') {
            return Some(hashes);
        }
    }
    None
}

fn ordered_item(text: &str) -> Option<&str> {
    let digits = text.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let rest = &text[digits..];
    rest.strip_prefix(". ")
}

fn inline(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    let mut plain = String::new();

    macro_rules! flush {
        () => {
            if !plain.is_empty() {
                out.push_str(&html_escape::encode_text(&plain));
                plain.clear();
            }
        };
    }

    while i < bytes.len() {
        let rest = &text[i..];

        if rest.starts_with("**") || rest.starts_with("__") {
            flush!();
            out.push_str("<strong>");
            i += 2;
            // close at the next marker
            if let Some(end) = text[i..].find(&rest[..2]) {
                out.push_str(&inline(&text[i..i + end]));
                i += end + 2;
            }
            out.push_str("</strong>");
            continue;
        }
        if rest.starts_with('*') || rest.starts_with('_') {
            let mark = &rest[..1];
            if let Some(end) = text[i + 1..].find(mark) {
                if end > 0 {
                    flush!();
                    out.push_str("<em>");
                    out.push_str(&inline(&text[i + 1..i + 1 + end]));
                    out.push_str("</em>");
                    i += end + 2;
                    continue;
                }
            }
        }
        if rest.starts_with('`') {
            if let Some(end) = text[i + 1..].find('`') {
                flush!();
                out.push_str("<code>");
                out.push_str(&html_escape::encode_text(&text[i + 1..i + 1 + end]));
                out.push_str("</code>");
                i += end + 2;
                continue;
            }
        }
        if rest.starts_with("~~") {
            if let Some(end) = text[i + 2..].find("~~") {
                flush!();
                out.push_str("<s>");
                out.push_str(&inline(&text[i + 2..i + 2 + end]));
                out.push_str("</s>");
                i += end + 4;
                continue;
            }
        }
        if rest.starts_with('[') {
            if let Some(close) = rest.find("](") {
                if let Some(paren) = rest[close + 2..].find(')') {
                    flush!();
                    out.push_str(&inline(&text[i + 1..i + close]));
                    i += close + 2 + paren + 1;
                    continue;
                }
            }
        }

        let ch = rest.chars().next().unwrap();
        plain.push(ch);
        i += ch.len_utf8();
    }

    flush!();
    out
}
