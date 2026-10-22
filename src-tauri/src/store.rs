use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::model::{Book, BookIndex, Bookmark, Library, Progress};

/// On-disk layout under the app data directory:
///
/// ```text
/// library.json
/// settings.json
/// books/<id>/index.json      parsed book: metadata, chapter list, toc
/// books/<id>/progress.json   where you stopped reading
/// books/<id>/bookmarks.json
/// books/<id>/ch/<n>.html     sanitized, ready-to-inject chapter markup
/// books/<id>/content/...     unpacked book assets (images, css we ignore)
/// books/<id>/source.epub     the original file, kept so the library is portable
/// ```
pub struct Store {
    pub root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Store { root }
    }

    pub fn books_dir(&self) -> PathBuf {
        self.root.join("books")
    }

    pub fn book_dir(&self, id: &str) -> PathBuf {
        self.books_dir().join(id)
    }

    pub fn library_path(&self) -> PathBuf {
        self.root.join("library.json")
    }

    pub fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn load_library(&self) -> Library {
        read_json(&self.library_path()).unwrap_or_default()
    }

    pub fn save_library(&self, lib: &Library) -> Result<(), String> {
        write_json(&self.library_path(), lib)
    }

    pub fn index_path(&self, id: &str) -> PathBuf {
        self.book_dir(id).join("index.json")
    }

    pub fn load_index(&self, id: &str) -> Option<BookIndex> {
        read_json(&self.index_path(id))
    }

    pub fn save_index(&self, index: &BookIndex) -> Result<(), String> {
        write_json(&self.index_path(&index.id), index)
    }

    pub fn progress_path(&self, id: &str) -> PathBuf {
        self.book_dir(id).join("progress.json")
    }

    pub fn load_progress(&self, id: &str) -> Progress {
        read_json(&self.progress_path(id)).unwrap_or_default()
    }

    pub fn save_progress(&self, id: &str, p: &Progress) -> Result<(), String> {
        write_json(&self.progress_path(id), p)
    }

    pub fn bookmarks_path(&self, id: &str) -> PathBuf {
        self.book_dir(id).join("bookmarks.json")
    }

    pub fn load_bookmarks(&self, id: &str) -> Vec<Bookmark> {
        read_json(&self.bookmarks_path(id)).unwrap_or_default()
    }

    pub fn save_bookmarks(&self, id: &str, marks: &[Bookmark]) -> Result<(), String> {
        write_json(&self.bookmarks_path(id), &marks)
    }

    pub fn chapter_path(&self, id: &str, index: i64) -> PathBuf {
        self.book_dir(id).join("ch").join(format!("{}.html", index))
    }

    pub fn content_dir(&self, id: &str) -> PathBuf {
        self.book_dir(id).join("content")
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        let dir = self.book_dir(id);
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| format!("could not delete {}: {}", dir.display(), e))?;
        }
        Ok(())
    }

    /// Books that were deleted by hand are dropped from the library.
    pub fn prune(&self, lib: &mut Library) {
        let before = lib.books.len();
        lib.books
            .retain(|b| self.index_path(&b.id).exists() && self.book_dir(&b.id).exists());
        if lib.books.len() != before {
            let _ = self.save_library(lib);
        }
    }
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    // Notepad and PowerShell save UTF-8 with a BOM; serde rejects that.
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    serde_json::from_str(text).ok()
}

/// Write via a temporary file and a rename so a crash mid-write cannot leave a
/// truncated index behind.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        f.flush().map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn view(store: &Store, book: &Book) -> crate::model::BookView {
    let p = store.load_progress(&book.id);
    crate::model::BookView {
        id: book.id.clone(),
        title: book.title.clone(),
        author: book.author.clone(),
        format: book.format.clone(),
        added: book.added,
        opened: book.opened,
        chars: book.chars,
        chapters: book.chapters,
        cover: book.cover.clone(),
        percent: p.percent,
        chapter: p.chapter,
        offset: p.offset,
        finished: p.percent >= 0.995,
    }
}
