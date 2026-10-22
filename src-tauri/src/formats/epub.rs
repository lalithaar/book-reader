use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use roxmltree::{Document, Node};

use super::{parent_dir, Built, IsNamed, Parsed};
use crate::html::{self, Ctx};
use crate::model::TocEntry;

struct Item {
    href: String,
    media: String,
    props: String,
}

pub fn import(id: &str, archive: &Path, content: &Path) -> Result<Parsed, String> {
    extract(archive, content)?;

    let container_path = content.join("META-INF").join("container.xml");
    let container = super::read_text(&container_path)
        .map_err(|_| "not an EPUB: META-INF/container.xml is missing".to_string())?;
    let opf_rel = rootfile(&container).ok_or("not an EPUB: container.xml has no rootfile")?;
    let opf_rel = decode_uri(&opf_rel);

    let opf_text = super::read_text(&content.join(&opf_rel))
        .map_err(|e| format!("cannot read package document: {}", e))?;
    let doc = Document::parse(&opf_text).map_err(|e| format!("cannot parse package document: {}", e))?;

    let opf_dir = parent_dir(&opf_rel);

    let mut items: HashMap<String, Item> = HashMap::new();
    let (mut title, mut author, mut language) = (String::new(), String::new(), String::new());

    for node in doc.descendants().filter(|n| n.is_element()) {
        match node.tag_name().name() {
            "title" if title.is_empty() => title = node_text(&node),
            "creator" if author.is_empty() => author = node_text(&node),
            "language" if language.is_empty() => language = node_text(&node),
            "item" => {
                if let (Some(item_id), Some(href)) = (node.attribute("id"), node.attribute("href")) {
                    items.insert(
                        item_id.to_string(),
                        Item {
                            href: href.to_string(),
                            media: node.attribute("media-type").unwrap_or("").to_lowercase(),
                            props: node.attribute("properties").unwrap_or("").to_string(),
                        },
                    );
                }
            }
            _ => {}
        }
    }

    let mut spine: Vec<String> = Vec::new();
    for node in doc.descendants().filter(|n| n.is_element() && n.named("itemref")) {
        if let Some(idref) = node.attribute("idref") {
            if let Some(item) = items.get(idref) {
                if item.media.contains("html") {
                    spine.push(item.href.clone());
                }
            }
        }
    }
    if spine.is_empty() {
        return Err("this EPUB declares no readable spine".into());
    }

    let nav_href = items
        .values()
        .find(|i| i.props.split_whitespace().any(|p| p == "nav"))
        .map(|i| i.href.clone());

    let ncx_href = doc
        .descendants()
        .find(|n| n.named("spine"))
        .and_then(|n| n.attribute("toc"))
        .and_then(|idref| items.get(idref))
        .map(|i| i.href.clone())
        .or_else(|| {
            items
                .values()
                .find(|i| i.media.contains("dtbncx"))
                .map(|i| i.href.clone())
        });

    // Every spine document, keyed by its path relative to the content root.
    let mut anchors: HashMap<String, usize> = HashMap::new();
    let mut paths: Vec<String> = Vec::new();
    for (i, href) in spine.iter().enumerate() {
        let rel = html::resolve(&opf_dir, &decode_uri(href));
        anchors.insert(rel.clone(), i);
        paths.push(rel);
    }

    let skip = nav_href
        .as_deref()
        .map(|h| html::resolve(&opf_dir, &decode_uri(h)))
        .unwrap_or_default();

    let toc = collect_toc(
        content,
        &opf_dir,
        &anchors,
        nav_href.as_deref(),
        ncx_href.as_deref(),
    );

    let prefix = format!("{}/content/", crate::asset_base(id));
    let mut chapters: Vec<Built> = Vec::new();

    for (i, rel) in paths.iter().enumerate() {
        if rel == &skip {
            continue;
        }
        let src = match super::read_text(&content.join(rel)) {
            Ok(text) => text,
            Err(_) => continue,
        };
        let body = html::extract_body(&src);
        let clean = html::sanitize(
            body,
            &Ctx {
                url_prefix: &prefix,
                base: &parent_dir(rel),
                anchors: &anchors,
            },
        );
        let title = toc
            .iter()
            .find(|t| t.chapter == i as i64)
            .map(|t| t.title.clone())
            .or_else(|| html::first_heading(&clean))
            .unwrap_or_default();
        chapters.push(Built::new(title, clean, i));
    }

    if chapters.is_empty() {
        return Err("this EPUB has no readable chapters".into());
    }

    let cover = find_cover(&doc, &items, &opf_dir)
        .map(|rel| format!("content/{}", html::escape_path(&rel)));

    Ok(Parsed {
        title: if title.is_empty() { super::file_stem(archive) } else { title },
        author: if author.is_empty() { "Unknown".into() } else { author },
        language,
        cover,
        chapters,
        toc,
    })
}

fn extract(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|e| format!("cannot open {}: {}", archive.display(), e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("not a readable EPUB: {}", e))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        if name.is_empty() || name.ends_with('/') {
            continue;
        }
        if name.starts_with('/') || name.contains("..") || name.contains(':') {
            continue;
        }
        let target = dest.join(&name);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        fs::write(&target, &bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn rootfile(xml: &str) -> Option<String> {
    let doc = Document::parse(xml).ok()?;
    doc.descendants()
        .find(|n| n.is_element() && n.named("rootfile"))
        .and_then(|n| n.attribute("full-path"))
        .map(|s| s.to_string())
}

fn find_cover(doc: &Document, items: &HashMap<String, Item>, opf_dir: &str) -> Option<String> {
    let by_meta = doc
        .descendants()
        .find(|n| n.named("meta") && n.attribute("name") == Some("cover"))
        .and_then(|n| n.attribute("content"))
        .and_then(|id| items.get(id))
        .map(|i| i.href.clone());

    let by_property = items
        .values()
        .find(|i| i.props.split_whitespace().any(|p| p == "cover-image"))
        .map(|i| i.href.clone());

    let by_name = items
        .values()
        .find(|i| i.media.starts_with("image/") && i.href.to_ascii_lowercase().contains("cover"))
        .map(|i| i.href.clone());

    by_meta
        .or(by_property)
        .or(by_name)
        .map(|href| html::resolve(opf_dir, &decode_uri(&href)))
}

fn collect_toc(
    content: &Path,
    opf_dir: &str,
    anchors: &HashMap<String, usize>,
    nav: Option<&str>,
    ncx: Option<&str>,
) -> Vec<TocEntry> {
    let mut out = Vec::new();
    if let Some(nav) = nav {
        if let Ok(text) = super::read_text(&content.join(html::resolve(opf_dir, &decode_uri(nav)))) {
            if let Ok(doc) = Document::parse(&text) {
                let root = doc
                    .descendants()
                    .find(|n| {
                        n.named("nav")
                            && n.attributes()
                                .any(|a| a.name() == "type" && a.value().split_whitespace().any(|t| t == "toc"))
                    })
                    .or_else(|| doc.descendants().find(|n| n.named("nav")));
                if let Some(root) = root {
                    let list = root
                        .descendants()
                        .find(|n| n.named("ol"))
                        .or(Some(root));
                    if let Some(list) = list {
                        walk_ol(list, 0, &mut out, opf_dir, anchors);
                    }
                }
            }
        }
    }

    if out.is_empty() {
        if let Some(ncx) = ncx {
            if let Ok(text) = super::read_text(&content.join(html::resolve(opf_dir, &decode_uri(ncx)))) {
                if let Ok(doc) = Document::parse(&text) {
                    for node in doc.descendants().filter(|n| n.named("navPoint")) {
                        walk_navpoint(node, 0, &mut out, opf_dir, anchors);
                    }
                }
            }
        }
    }

    out
}

fn walk_ol(list: Node, depth: i64, out: &mut Vec<TocEntry>, opf_dir: &str, anchors: &HashMap<String, usize>) {
    for li in list.children().filter(|c| c.named("li")) {
        if let Some(link) = li.children().find(|c| c.named("a")) {
            if let Some(href) = link.attribute("href") {
                let label = html::collapse(&node_text(&link));
                if let Some(chapter) = chapter_for(href, opf_dir, anchors) {
                    out.push(TocEntry {
                        title: if label.is_empty() {
                            format!("Chapter {}", chapter + 1)
                        } else {
                            label
                        },
                        chapter: chapter as i64,
                        depth,
                    });
                }
            }
        }
        for nested in li.children().filter(|c| c.named("ol")) {
            walk_ol(nested, depth + 1, out, opf_dir, anchors);
        }
    }
}

fn walk_navpoint(node: Node, depth: i64, out: &mut Vec<TocEntry>, opf_dir: &str, anchors: &HashMap<String, usize>) {
    let label = node
        .children()
        .find(|c| c.named("navLabel"))
        .and_then(|n| n.descendants().find(|d| d.named("text")))
        .map(|n| html::collapse(n.text().unwrap_or("")))
        .unwrap_or_default();

    let src = node
        .children()
        .find(|c| c.named("content"))
        .and_then(|n| n.attribute("src"));

    if let Some(src) = src {
        if let Some(chapter) = chapter_for(src, opf_dir, anchors) {
            out.push(TocEntry {
                title: if label.is_empty() {
                    format!("Chapter {}", chapter + 1)
                } else {
                    label
                },
                chapter: chapter as i64,
                depth,
            });
        }
    }

    for child in node.children().filter(|c| c.named("navPoint")) {
        walk_navpoint(child, depth + 1, out, opf_dir, anchors);
    }
}

fn chapter_for(href: &str, opf_dir: &str, anchors: &HashMap<String, usize>) -> Option<usize> {
    let path = href.split('#').next().unwrap_or(href);
    if path.is_empty() {
        return None;
    }
    anchors
        .get(&html::resolve(opf_dir, &decode_uri(path)))
        .copied()
}

fn node_text(node: &Node) -> String {
    let mut out = String::new();
    for descendant in node.descendants().filter(|n| n.is_text()) {
        if let Some(text) = descendant.text() {
            out.push_str(text);
            out.push(' ');
        }
    }
    html::collapse(&out)
}

fn decode_uri(value: &str) -> String {
    percent_encoding::percent_decode_str(value)
        .decode_utf8_lossy()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{make_epub, scratch};

    #[test]
    fn imports_metadata_spine_and_sanitizes_markup() {
        let dir = scratch("epub");
        let archive = dir.join("book.epub");
        make_epub(&archive);
        let content = dir.join("content");

        let parsed = import("abc123", &archive, &content).unwrap();

        assert_eq!(parsed.title, "Test Book");
        assert_eq!(parsed.author, "Ada Lovelace");
        assert_eq!(parsed.language, "en");
        assert_eq!(parsed.chapters.len(), 2);
        assert_eq!(parsed.chapters[0].key, 0);
        assert_eq!(parsed.chapters[1].key, 1);

        let first = &parsed.chapters[0].html;
        assert!(first.contains("<h1>One</h1>"));
        assert!(first.contains("<em>world</em>"));
        assert!(!first.contains("<script"));
        assert!(!first.contains("alert(1)"));
        assert!(!first.contains("onclick"));
        assert!(!first.contains("<style"));
        assert!(
            first.contains("http://book.localhost/abc123/content/OEBPS/Images/pic.png"),
            "image url rewritten: {}",
            first
        );
        assert!(first.contains("href=\"#ch:1\""), "cross chapter link: {}", first);
        assert!(!first.contains("example.com"), "external link dropped: {}", first);

        assert_eq!(parsed.toc.len(), 2);
        assert_eq!(parsed.toc[0].chapter, 0);
        assert_eq!(parsed.toc[1].title, "Chapter Two");
        assert_eq!(parsed.toc[1].chapter, 1);

        assert_eq!(
            parsed.cover.as_deref(),
            Some("content/OEBPS/Images/cover.png")
        );
    }

    #[test]
    fn chapter_files_and_index_round_trip() {
        let dir = scratch("assemble");
        let archive = dir.join("book.epub");
        make_epub(&archive);
        let content = dir.join("content");
        let parsed = import("abc123", &archive, &content).unwrap();

        let store = crate::store::Store::new(dir.join("data"));
        let index = crate::model::BookIndex {
            id: "abc123".into(),
            title: parsed.title.clone(),
            author: parsed.author.clone(),
            language: parsed.language.clone(),
            format: "epub".into(),
            total_chars: 0,
            chapters: Vec::new(),
            toc: parsed.toc.clone(),
        };

        let index = crate::formats::assemble(&store, index, parsed.chapters).unwrap();
        assert_eq!(index.chapters.len(), 2);
        assert!(index.total_chars > 20);
        assert_eq!(index.chapters[1].before, index.chapters[0].chars);

        let written = fs::read_to_string(store.chapter_path("abc123", 0)).unwrap();
        assert!(written.contains("Hello"));

        store.save_index(&index).unwrap();
        let reloaded = store.load_index("abc123").unwrap();
        assert_eq!(reloaded.chapters.len(), 2);
        assert_eq!(reloaded.toc.len(), 2);
    }

    #[test]
    fn rejects_a_zip_that_is_not_an_epub() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let dir = scratch("broken");
        let archive = dir.join("book.epub");
        let file = fs::File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("hello.txt", SimpleFileOptions::default()).unwrap();
        zip.write_all(b"not a book").unwrap();
        zip.finish().unwrap();

        let error = import("x", &archive, &dir.join("content")).unwrap_err();
        assert!(error.contains("EPUB"), "unexpected error: {}", error);
    }
}
