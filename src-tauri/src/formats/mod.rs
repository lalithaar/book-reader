use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::html;
use crate::model::{BookIndex, ChapterMeta, TocEntry};
use crate::store::Store;

pub mod epub;
pub mod fb2;
pub mod plain;

/// Element testing that is safe on text and comment nodes: `tag_name()` panics
/// on anything that is not an element, and TOC/spine walks do hit those.
pub trait IsNamed {
    fn named(&self, name: &str) -> bool;
}

impl IsNamed for roxmltree::Node<'_, '_> {
    fn named(&self, name: &str) -> bool {
        self.is_element() && self.tag_name().name() == name
    }
}

/// Result of parsing one book file, before chapter files are written.
#[derive(Debug)]
pub struct Parsed {
    pub title: String,
    pub author: String,
    pub language: String,
    /// Path inside the book directory, e.g. `content/OEBPS/cover.jpg`.
    pub cover: Option<String>,
    pub chapters: Vec<Built>,
    pub toc: Vec<TocEntry>,
}

/// A chapter ready to be written to disk. `key` is the position in the source
/// spine so that TOC entries can be remapped after empty chapters are dropped.
#[derive(Debug)]
pub struct Built {
    pub title: String,
    pub html: String,
    pub key: usize,
}

impl Built {
    pub fn new(title: impl Into<String>, html: String, key: usize) -> Self {
        Built {
            title: title.into(),
            html,
            key,
        }
    }
}

/// Write chapter files and finish the index. Empty chapters are discarded and
/// TOC entries pointing at them are dropped, so page numbers stay contiguous.
pub fn assemble(store: &Store, mut index: BookIndex, chapters: Vec<Built>) -> Result<BookIndex, String> {
    let dir = store.book_dir(&index.id).join("ch");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let mut metas: Vec<ChapterMeta> = Vec::new();
    let mut remap: HashMap<usize, usize> = HashMap::new();
    let mut before: i64 = 0;

    for chapter in chapters {
        let chars = html::text_of(&chapter.html).chars().count() as i64;
        if chars == 0 {
            continue;
        }
        let number = metas.len();
        remap.insert(chapter.key, number);

        let title = {
            let trimmed = chapter.title.trim();
            if trimmed.is_empty() {
                format!("Chapter {}", number + 1)
            } else {
                trimmed.to_string()
            }
        };

        let path = dir.join(format!("{}.html", number));
        fs::write(&path, chapter.html.as_bytes()).map_err(|e| e.to_string())?;

        metas.push(ChapterMeta {
            title,
            chars,
            before,
        });
        before += chars;
    }

    if metas.is_empty() {
        return Err("this book has no readable text".into());
    }

    let mut toc: Vec<TocEntry> = Vec::new();
    for entry in index.toc.drain(..) {
        if let Some(&chapter) = remap.get(&(entry.chapter as usize)) {
            toc.push(TocEntry {
                chapter: chapter as i64,
                ..entry
            });
        }
    }
    if toc.is_empty() {
        toc = metas
            .iter()
            .enumerate()
            .map(|(i, m)| TocEntry {
                title: m.title.clone(),
                chapter: i as i64,
                depth: 0,
            })
            .collect();
    }

    index.chapters = metas;
    index.toc = toc;
    index.total_chars = before;
    Ok(index)
}

pub fn read_text(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    Ok(decode(&bytes))
}

/// UTF-8 when possible, Windows-1251 otherwise (most non-UTF-8 books in the
/// wild are Russian FB2), with a final lossy pass so nothing is ever fatal.
pub fn decode(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let encoding = if bytes[0] == 0xFF {
            encoding_rs::UTF_16LE
        } else {
            encoding_rs::UTF_16BE
        };
        let (text, _, _) = encoding.decode(&bytes[2..]);
        return text.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => {
            let (text, _, _) = encoding_rs::WINDOWS_1251.decode(bytes);
            if text.contains('\u{FFFD}') {
                String::from_utf8_lossy(bytes).into_owned()
            } else {
                text.into_owned()
            }
        }
    }
}

pub fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

pub fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Untitled".into())
}
