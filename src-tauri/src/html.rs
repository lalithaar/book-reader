//! A deliberately small, tolerant HTML rewriter.
//!
//! Book markup is hostile in practice: unclosed tags, stray entities, inline
//! scripts. Instead of a full parser we tokenise, drop anything that can
//! execute or re-style the page, and rewrite resource URLs so the webview can
//! fetch them through our own protocol. Output is plain, flattened HTML that
//! the reader's CSS owns completely.

use std::collections::HashMap;

use html_escape::decode_html_entities;
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};

/// Characters that must never survive into a URL we hand to the webview.
pub const PATH_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'#')
    .add(b'%')
    .add(b'?')
    .add(b'&')
    .add(b'{')
    .add(b'}')
    .add(b'|')
    .add(b'\\')
    .add(b'^')
    .add(b'[')
    .add(b']')
    .add(b'\'');

/// Elements removed together with their contents.
const DROP: &[&str] = &[
    "script", "style", "iframe", "object", "embed", "applet", "video", "audio", "canvas",
    "form", "button", "select", "textarea", "svg", "math", "template", "noscript", "head",
    "title", "meta", "link", "base", "frame", "frameset", "keygen", "plaintext",
];

/// Removed elements that never have a closing tag.
const VOID_DROP: &[&str] = &["input", "meta", "link", "base", "param", "source", "track", "embed", "frame", "keygen"];

/// Elements passed through. Everything else is unwrapped (children survive).
const ALLOWED: &[&str] = &[
    "a", "abbr", "address", "article", "aside", "b", "blockquote", "br", "caption", "cite",
    "code", "col", "colgroup", "dd", "del", "dfn", "div", "dl", "dt", "em", "figcaption",
    "figure", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "i", "img", "ins", "kbd", "li",
    "mark", "ol", "p", "pre", "q", "rp", "rt", "ruby", "s", "samp", "section", "small",
    "span", "strong", "sub", "sup", "table", "tbody", "td", "tfoot", "th", "thead", "tr",
    "u", "ul", "var", "wbr",
];

pub struct Ctx<'a> {
    /// Base for asset URLs, e.g. `http://book.localhost/<id>/content/`.
    pub url_prefix: &'a str,
    /// Directory of the document being rewritten, relative to the content root.
    pub base: &'a str,
    /// Content-root path of every spine document -> chapter number.
    pub anchors: &'a HashMap<String, usize>,
}

struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
    closing: bool,
    self_closing: bool,
}

pub fn escape_path(path: &str) -> String {
    utf8_percent_encode(path, PATH_SET).to_string()
}

pub fn sanitize(src: &str, ctx: &Ctx) -> String {
    let mut out = String::with_capacity(src.len() + 64);
    let bytes = src.as_bytes();
    let mut i = 0usize;
    let mut skip: Vec<String> = Vec::new();

    while i < bytes.len() {
        if bytes[i] != b'<' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'<' {
                i += 1;
            }
            if skip.is_empty() {
                out.push_str(&src[start..i]);
            }
            continue;
        }

        if src[i..].starts_with("<!--") {
            i = match src[i..].find("-->") {
                Some(p) => i + p + 3,
                None => bytes.len(),
            };
            continue;
        }
        if src[i..].starts_with("<!") || src[i..].starts_with("<?") {
            i = match src[i..].find('>') {
                Some(p) => i + p + 1,
                None => bytes.len(),
            };
            continue;
        }

        let (tag, next) = match parse_tag(src, i) {
            Some(v) => v,
            None => {
                if skip.is_empty() {
                    out.push_str("&lt;");
                }
                i += 1;
                continue;
            }
        };
        i = next;

        let name = tag.name.to_ascii_lowercase();

        if !skip.is_empty() {
            if !tag.closing && is_drop(&name) && !tag.self_closing && !is_void_drop(&name) {
                skip.push(name);
            } else if tag.closing && skip.last().map(|s| s == &name).unwrap_or(false) {
                skip.pop();
            }
            continue;
        }

        if is_drop(&name) {
            if !tag.closing && !tag.self_closing && !is_void_drop(&name) {
                skip.push(name);
            }
            continue;
        }

        if tag.closing {
            if is_allowed(&name) {
                out.push_str("</");
                out.push_str(&name);
                out.push('>');
            }
            continue;
        }

        if !is_allowed(&name) {
            continue;
        }

        emit_tag(&mut out, &name, &tag, ctx);
    }

    out
}

fn emit_tag(out: &mut String, name: &str, tag: &Tag, ctx: &Ctx) {
    out.push('<');
    out.push_str(name);
    for (key, value) in &tag.attrs {
        let key = key.to_ascii_lowercase();
        if key.starts_with("on") || key == "style" || key == "srcset" || key == "ping" {
            continue;
        }
        match key.as_str() {
            "id" | "class" | "lang" | "dir" | "title" | "alt" | "colspan" | "rowspan"
            | "start" | "reversed" | "scope" | "datetime" | "abbr" | "value" => {
                out.push(' ');
                out.push_str(&key);
                out.push_str("=\"");
                escape_attr(out, value);
                out.push('"');
            }
            "width" | "height" => {
                let numeric = !value.is_empty() && value.chars().all(|c| c.is_ascii_digit() || c == '.');
                if numeric && (name == "img" || name == "td" || name == "th") {
                    out.push(' ');
                    out.push_str(&key);
                    out.push_str("=\"");
                    out.push_str(value);
                    out.push('"');
                }
            }
            "src" | "href" | "poster" => {
                if let Some(url) = rewrite_url(value, ctx) {
                    out.push(' ');
                    out.push_str(&key);
                    out.push_str("=\"");
                    escape_attr(out, &url);
                    out.push('"');
                }
            }
            _ => {
                if key.ends_with(":href") {
                    if let Some(url) = rewrite_url(value, ctx) {
                        out.push_str(" src=\"");
                        escape_attr(out, &url);
                        out.push('"');
                    }
                }
            }
        }
    }
    out.push('>');
}

fn escape_attr(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

fn rewrite_url(raw: &str, ctx: &Ctx) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("data:") {
        return Some(value.to_string());
    }
    if lower.starts_with("javascript:") || lower.starts_with("vbscript:") || lower.starts_with("file:") {
        return None;
    }
    if let Some(rest) = value.strip_prefix('#') {
        return Some(format!("#{}", rest));
    }

    let (path, fragment) = match value.find('#') {
        Some(k) => (&value[..k], Some(&value[k + 1..])),
        None => (value, None),
    };

    if let Some(colon) = path.find(':') {
        let scheme = &path[..colon];
        if scheme.len() > 1 && !scheme.contains('/') && !scheme.contains('.') {
            return None; // http(s), mailto, and friends: dropped
        }
    }
    if path.is_empty() {
        return fragment.map(|f| format!("#{}", f));
    }

    let stripped = match path.find('?') {
        Some(q) => &path[..q],
        None => path,
    };
    let decoded = percent_decode_str(stripped).decode_utf8_lossy().to_string();
    let relative = resolve(ctx.base, &decoded);

    if is_document(&relative) {
        return ctx.anchors.get(&relative).map(|idx| format!("#ch:{}", idx));
    }

    Some(format!("{}{}", ctx.url_prefix, escape_path(&relative)))
}

fn is_document(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".xhtml") || lower.ends_with(".html") || lower.ends_with(".htm")
}

/// Resolve `rel` against `base`, both relative to the content root.
pub fn resolve(base: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if !rel.starts_with('/') {
        for segment in base.split('/') {
            if !segment.is_empty() {
                parts.push(segment);
            }
        }
    }
    for segment in rel.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn is_drop(name: &str) -> bool {
    DROP.contains(&name)
}

fn is_void_drop(name: &str) -> bool {
    VOID_DROP.contains(&name)
}

fn is_allowed(name: &str) -> bool {
    ALLOWED.contains(&name)
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b':' || b == b'-' || b == b'_'
}

fn parse_tag(src: &str, start: usize) -> Option<(Tag, usize)> {
    let bytes = src.as_bytes();
    let mut i = start + 1;
    let closing = i < bytes.len() && bytes[i] == b'/';
    if closing {
        i += 1;
    }
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let name_start = i;
    while i < bytes.len() && is_name_byte(bytes[i]) {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name = src[name_start..i].to_string();

    let mut attrs: Vec<(String, String)> = Vec::new();
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        if bytes[i] == b'>' {
            i += 1;
            break;
        }
        if bytes[i] == b'/' {
            i += 1;
            continue;
        }
        let key_start = i;
        while i < bytes.len()
            && bytes[i] != b'='
            && bytes[i] != b'>'
            && bytes[i] != b'/'
            && !bytes[i].is_ascii_whitespace()
        {
            i += 1;
        }
        if i == key_start {
            i += 1;
            continue;
        }
        let key = src[key_start..i].to_string();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() {
                return None;
            }
            let value = if bytes[i] == b'"' || bytes[i] == b'\'' {
                let quote = bytes[i];
                i += 1;
                let value_start = i;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                let v = src[value_start..i].to_string();
                if i < bytes.len() {
                    i += 1;
                }
                v
            } else {
                let value_start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                    i += 1;
                }
                src[value_start..i].to_string()
            };
            attrs.push((key, value));
        } else {
            attrs.push((key, String::new()));
        }
    }

    let raw = &src[start..i];
    let self_closing = raw
        .trim_end()
        .trim_end_matches('>')
        .trim_end()
        .ends_with('/');

    Some((
        Tag {
            name,
            attrs,
            closing,
            self_closing,
        },
        i,
    ))
}

/// Grab the inside of `<body>`, falling back to the whole document.
pub fn extract_body(src: &str) -> &str {
    let probe = src.to_ascii_lowercase();
    let start = match probe.find("<body") {
        Some(p) => match probe[p..].find('>') {
            Some(q) => p + q + 1,
            None => return src,
        },
        None => return src,
    };
    let end = match probe[start..].find("</body") {
        Some(q) => start + q,
        None => src.len(),
    };
    &src[start..end.max(start)]
}

fn normalize_newlines(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n")
}

/// Text content of a document, decoded exactly the way the browser will decode
/// it. Character offsets measured here are the offsets the UI reports back.
pub fn text_of(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let bytes = html.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if html[i..].starts_with("<!--") {
                i = match html[i..].find("-->") {
                    Some(p) => i + p + 3,
                    None => bytes.len(),
                };
                continue;
            }
            i = match html[i..].find('>') {
                Some(p) => i + p + 1,
                None => bytes.len(),
            };
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] != b'<' {
            i += 1;
        }
        out.push_str(&html[start..i]);
    }
    normalize_newlines(&decode_html_entities(&out))
}

/// First heading in a fragment, used as a fallback chapter label.
pub fn first_heading(html: &str) -> Option<String> {
    for tag in ["<h1", "<h2", "<h3", "<title"] {
        if let Some(start) = html.find(tag) {
            if let Some(open) = html[start..].find('>') {
                let from = start + open + 1;
                let close = format!("</{}", &tag[1..]);
                if let Some(end) = html[from..].find(&close) {
                    let text = collapse(&text_of(&html[from..from + end]));
                    if !text.is_empty() && text.chars().count() <= 120 {
                        return Some(text);
                    }
                }
            }
        }
    }
    None
}

pub fn collapse(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut space = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            space = !out.is_empty();
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(url: &'a str, base: &'a str, anchors: &'a HashMap<String, usize>) -> Ctx<'a> {
        Ctx {
            url_prefix: url,
            base,
            anchors,
        }
    }

    #[test]
    fn drops_scripts_and_handlers() {
        let anchors = HashMap::new();
        let c = ctx("http://b/", "", &anchors);
        let out = sanitize(
            "<p onclick=\"x()\">hi<script>alert(1)</script><iframe src=\"y\"></iframe></p>",
            &c,
        );
        assert_eq!(out, "<p>hi</p>");
    }

    #[test]
    fn rewrites_relative_images() {
        let anchors = HashMap::new();
        let c = ctx("http://b/content/", "OEBPS/Text", &anchors);
        let out = sanitize("<img src=\"../Images/a b.png\"/>", &c);
        assert_eq!(out, "<img src=\"http://b/content/OEBPS/Images/a%20b.png\">");
    }

    #[test]
    fn maps_cross_chapter_links() {
        let mut anchors = HashMap::new();
        anchors.insert("OEBPS/Notes.xhtml".to_string(), 7usize);
        let c = ctx("http://b/", "OEBPS/Text", &anchors);
        assert_eq!(sanitize("<a href=\"../Notes.xhtml#n1\">n</a>", &c), "<a href=\"#ch:7\">n</a>");
        assert_eq!(sanitize("<a href=\"https://x.test\">x</a>", &c), "<a>x</a>");
    }

    #[test]
    fn text_offsets_survive_entities() {
        assert_eq!(text_of("<p>a &amp; b</p>"), "a & b");
        assert_eq!(text_of("<p>x\r\ny</p>"), "x\ny");
    }
}
