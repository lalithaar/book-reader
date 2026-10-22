use std::collections::HashMap;
use std::fs;
use std::path::Path;

use base64::Engine as _;
use roxmltree::{Document, Node};

use super::{Built, IsNamed, Parsed};
use crate::html;

struct Ctx {
    images: HashMap<String, String>,
}

pub fn import(id: &str, path: &Path, content: &Path) -> Result<Parsed, String> {
    let text = super::read_text(path)?;
    let doc = Document::parse(&text).map_err(|e| format!("cannot parse FB2: {}", e))?;

    let prefix = format!("{}/content/", crate::asset_base(id));
    let images = unpack_images(&doc, content, &prefix)?;

    let description = doc
        .descendants()
        .find(|n| n.named("description"));

    let title = description
        .and_then(|d| d.descendants().find(|n| n.tag_name().name() == "book-title"))
        .map(|n| html::collapse(&collect_text(&n)))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| super::file_stem(path));

    let author = description
        .and_then(|d| d.descendants().find(|n| n.tag_name().name() == "title-info"))
        .and_then(|ti| ti.children().find(|c| c.named("author")))
        .map(|a| author_name(&a))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown".into());

    let language = description
        .and_then(|d| d.descendants().find(|n| n.named("lang")))
        .map(|n| n.text().unwrap_or("").trim().to_string())
        .unwrap_or_default();

    let cover = description
        .and_then(|d| d.descendants().find(|n| n.named("coverpage")))
        .and_then(|c| c.descendants().find(|n| n.named("image")))
        .and_then(|img| image_href(&img))
        .and_then(|key| images.get(&key).cloned())
        .map(|url| url.replacen(&format!("{}/content/", crate::asset_base(id)), "content/", 1));

    let ctx = Ctx { images };
    let mut chapters = Vec::new();
    let mut toc = Vec::new();

    let bodies: Vec<Node> = doc
        .root_element()
        .children()
        .filter(|n| n.is_element() && n.named("body"))
        .collect();

    for body in &bodies {
        let sections: Vec<Node> = body
            .children()
            .filter(|n| n.is_element() && n.named("section"))
            .collect();

        if sections.is_empty() {
            let mut out = String::new();
            render(body, &mut out, &ctx, 1);
            chapters.push(Built::new(String::new(), out, chapters.len()));
            continue;
        }

        for section in sections {
            let label = section
                .children()
                .find(|n| n.named("title"))
                .map(|t| html::collapse(&collect_text(&t)))
                .unwrap_or_default();
            let mut out = String::new();
            render(&section, &mut out, &ctx, 1);
            let key = chapters.len();
            if label.is_empty() {
                chapters.push(Built::new(String::new(), out, key));
            } else {
                toc.push(crate::model::TocEntry {
                    title: label.clone(),
                    chapter: key as i64,
                    depth: 0,
                });
                chapters.push(Built::new(label, out, key));
            }
        }
    }

    if chapters.is_empty() {
        return Err("this FB2 file has no text".into());
    }

    Ok(Parsed {
        title,
        author,
        language,
        cover,
        chapters,
        toc,
    })
}

fn unpack_images(doc: &Document, content: &Path, prefix: &str) -> Result<HashMap<String, String>, String> {
    let dir = content.join("img");
    let mut map = HashMap::new();
    for node in doc.descendants().filter(|n| n.named("binary")) {
        let id = match node.attribute("id") {
            Some(id) => id,
            None => continue,
        };
        let mime = node.attribute("content-type").unwrap_or("image/jpeg");
        let raw: String = node
            .text()
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if raw.is_empty() {
            continue;
        }
        let bytes = match base64::engine::general_purpose::STANDARD.decode(raw) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let ext = match mime.to_ascii_lowercase().as_str() {
            "image/png" => "png",
            "image/gif" => "gif",
            "image/svg+xml" => "svg",
            "image/webp" => "webp",
            _ => "jpg",
        };
        let name = format!("{}.{}", safe_name(id), ext);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        fs::write(dir.join(&name), &bytes).map_err(|e| e.to_string())?;
        map.insert(id.to_string(), format!("{}img/{}", prefix, html::escape_path(&name)));
    }
    Ok(map)
}

fn safe_name(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

fn author_name(author: &Node) -> String {
    let mut parts = Vec::new();
    for tag in ["first-name", "middle-name", "last-name", "nickname"] {
        if let Some(n) = author.children().find(|c| c.tag_name().name() == tag) {
            let text = html::collapse(n.text().unwrap_or(""));
            if !text.is_empty() {
                parts.push(text);
            }
        }
    }
    parts.join(" ")
}

fn collect_text(node: &Node) -> String {
    let mut out = String::new();
    for descendant in node.descendants().filter(|n| n.is_text()) {
        if let Some(text) = descendant.text() {
            out.push_str(text);
            out.push(' ');
        }
    }
    out
}

fn image_href(node: &Node) -> Option<String> {
    node.attributes()
        .find(|a| a.name() == "href")
        .map(|a| a.value().trim_start_matches('#').to_string())
}

fn render(node: &Node, out: &mut String, ctx: &Ctx, depth: usize) {
    for child in node.children() {
        if child.is_text() {
            out.push_str(&html_escape::encode_text(child.text().unwrap_or("")));
        } else if child.is_element() {
            render_element(&child, out, ctx, depth);
        }
    }
}

fn tag(node: &Node, out: &mut String, ctx: &Ctx, depth: usize, open: &str, close: &str) {
    out.push_str(open);
    render(node, out, ctx, depth);
    out.push_str(close);
}

fn render_element(node: &Node, out: &mut String, ctx: &Ctx, depth: usize) {
    match node.tag_name().name() {
        "section" => tag(node, out, ctx, depth + 1, "<section>", "</section>"),
        "title" => {
            let t = node
                .children()
                .find(|c| c.named("p"))
                .map(|p| html::collapse(&collect_text(&p)))
                .unwrap_or_else(|| html::collapse(&collect_text(node)));
            let level = if depth <= 1 { "h2" } else { "h3" };
            out.push_str(&format!("<{}>{}</{}>", level, html_escape::encode_text(&t), level));
        }
        "subtitle" => {
            let t = html::collapse(&collect_text(node));
            out.push_str(&format!("<h3>{}</h3>", html_escape::encode_text(&t)));
        }
        "epigraph" => tag(node, out, ctx, depth, "<blockquote>", "</blockquote>"),
        "cite" => tag(node, out, ctx, depth, "<blockquote>", "</blockquote>"),
        "annotation" => tag(node, out, ctx, depth, "<div class=\"annotation\">", "</div>"),
        "poem" => tag(node, out, ctx, depth, "<div class=\"poem\">", "</div>"),
        "stanza" => tag(node, out, ctx, depth, "<div class=\"stanza\">", "</div>"),
        "v" => tag(node, out, ctx, depth, "<p class=\"v\">", "</p>"),
        "text-author" => tag(node, out, ctx, depth, "<p class=\"by\">", "</p>"),
        "date" => tag(node, out, ctx, depth, "<p class=\"by\">", "</p>"),
        "p" => tag(node, out, ctx, depth, "<p>", "</p>"),
        "emphasis" => tag(node, out, ctx, depth, "<em>", "</em>"),
        "strong" => tag(node, out, ctx, depth, "<strong>", "</strong>"),
        "strikethrough" => tag(node, out, ctx, depth, "<s>", "</s>"),
        "sub" => tag(node, out, ctx, depth, "<sub>", "</sub>"),
        "sup" => tag(node, out, ctx, depth, "<sup>", "</sup>"),
        "code" => tag(node, out, ctx, depth, "<code>", "</code>"),
        "style" => tag(node, out, ctx, depth, "<em>", "</em>"),
        "empty-line" => out.push_str("<br>"),
        "a" => {
            let href = node
                .attributes()
                .find(|a| a.name() == "href")
                .map(|a| a.value().to_string())
                .unwrap_or_default();
            if href.starts_with('#') {
                out.push_str("<a href=\"");
                out.push_str(&html_escape::encode_double_quoted_attribute(&href));
                out.push_str("\">");
                render(node, out, ctx, depth);
                out.push_str("</a>");
            } else {
                render(node, out, ctx, depth);
            }
        }
        "image" => {
            if let Some(key) = image_href(node) {
                if let Some(url) = ctx.images.get(&key) {
                    out.push_str(&format!("<img src=\"{}\" alt=\"\">", url));
                }
            }
        }
        "table" => tag(node, out, ctx, depth, "<table>", "</table>"),
        "tr" => tag(node, out, ctx, depth, "<tr>", "</tr>"),
        "td" => tag(node, out, ctx, depth, "<td>", "</td>"),
        "th" => tag(node, out, ctx, depth, "<th>", "</th>"),
        _ => render(node, out, ctx, depth),
    }
}
