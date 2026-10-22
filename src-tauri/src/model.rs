use serde::{Deserialize, Serialize};

/// A book as stored in `library.json`. Deliberately free of reading state:
/// position lives in each book's own `progress.json` so that turning a page
/// never rewrites the whole library.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Book {
    pub id: String,
    pub title: String,
    pub author: String,
    /// "epub" | "fb2" | "txt" | "md"
    pub format: String,
    /// Where the user opened it from, for reference only.
    pub source: String,
    pub added: i64,
    pub opened: i64,
    pub chars: i64,
    pub chapters: i64,
    /// Path inside the book directory, URL-escaped, e.g. `content/OEBPS/cover.jpg`.
    pub cover: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Library {
    #[serde(default)]
    pub books: Vec<Book>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Progress {
    pub chapter: i64,
    pub offset: i64,
    pub percent: f64,
    pub updated: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ChapterMeta {
    pub title: String,
    pub chars: i64,
    /// Characters in all preceding chapters, so the UI can show whole-book progress.
    pub before: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct TocEntry {
    pub title: String,
    pub chapter: i64,
    pub depth: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct BookIndex {
    pub id: String,
    pub title: String,
    pub author: String,
    pub language: String,
    pub format: String,
    pub total_chars: i64,
    pub chapters: Vec<ChapterMeta>,
    pub toc: Vec<TocEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Bookmark {
    pub chapter: i64,
    pub offset: i64,
    pub percent: f64,
    pub label: String,
    pub created: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Hit {
    pub chapter: i64,
    pub offset: i64,
    pub percent: f64,
    pub snippet: String,
}

/// What the library screen renders: metadata plus where you left off.
#[derive(Serialize, Clone, Debug)]
pub struct BookView {
    pub id: String,
    pub title: String,
    pub author: String,
    pub format: String,
    pub added: i64,
    pub opened: i64,
    pub chars: i64,
    pub chapters: i64,
    pub cover: Option<String>,
    pub percent: f64,
    pub chapter: i64,
    pub offset: i64,
    pub finished: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct OpenPayload {
    pub book: BookView,
    pub index: BookIndex,
    pub progress: Progress,
    pub bookmarks: Vec<Bookmark>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ChapterPayload {
    pub html: String,
    pub title: String,
    pub chars: i64,
    pub before: i64,
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
