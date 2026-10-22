use std::fs;
use std::path::Path;

use crate::formats;
use crate::model::{now, Book, BookIndex, Library};
use crate::store::Store;

pub fn import_file(store: &Store, path: &Path, library: &mut Library) -> Result<Book, String> {
    let source = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let source_string = source.to_string_lossy().to_string();

    if let Some(existing) = library
        .books
        .iter()
        .find(|b| b.source.eq_ignore_ascii_case(&source_string))
    {
        return Ok(existing.clone());
    }

    let format = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let format = if format == "markdown" {
        "md".to_string()
    } else {
        format
    };

    let id = uuid::Uuid::new_v4().simple().to_string();
    let dir = store.book_dir(&id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let copy = dir.join(format!("source.{}", format));
    fs::copy(&source, &copy).map_err(|e| format!("cannot read {}: {}", source.display(), e))?;

    let content = store.content_dir(&id);
    let parsed = match format.as_str() {
        "epub" => formats::epub::import(&id, &copy, &content),
        "fb2" => formats::fb2::import(&id, &copy, &content),
        "md" => formats::plain::import_md(&copy),
        "txt" => formats::plain::import_txt(&copy),
        other => Err(format!(".{} files are not supported yet", other)),
    };

    let parsed = match parsed {
        Ok(parsed) => parsed,
        Err(error) => {
            let _ = store.remove(&id);
            return Err(error);
        }
    };

    let formats::Parsed {
        title,
        author,
        language,
        cover,
        chapters,
        toc,
    } = parsed;

    let index = BookIndex {
        id: id.clone(),
        title,
        author,
        language,
        format: format.clone(),
        total_chars: 0,
        chapters: Vec::new(),
        toc,
    };

    let index = match formats::assemble(store, index, chapters) {
        Ok(index) => index,
        Err(error) => {
            let _ = store.remove(&id);
            return Err(error);
        }
    };

    if let Err(error) = store.save_index(&index) {
        let _ = store.remove(&id);
        return Err(error);
    }

    let book = Book {
        id,
        title: index.title.clone(),
        author: index.author.clone(),
        format,
        source: source_string,
        added: now(),
        opened: 0,
        chars: index.total_chars,
        chapters: index.chapters.len() as i64,
        cover,
    };

    library.books.push(book.clone());
    store.save_library(library)?;
    Ok(book)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_a_book_into_the_library() {
        let dir = crate::testutil::scratch("import");
        let archive = dir.join("book.epub");
        crate::testutil::make_epub(&archive);

        let store = Store::new(dir.join("data"));
        let mut library = Library::default();

        let book = import_file(&store, &archive, &mut library).unwrap();
        assert_eq!(book.title, "Test Book");
        assert_eq!(book.author, "Ada Lovelace");
        assert_eq!(book.format, "epub");
        assert_eq!(book.chapters, 2);
        assert!(book.chars > 0);
        assert_eq!(book.cover.as_deref(), Some("content/OEBPS/Images/cover.png"));
        assert!(store.chapter_path(&book.id, 0).exists());
        assert!(store.chapter_path(&book.id, 1).exists());
        assert!(store.load_index(&book.id).is_some());

        // The library is written through to disk.
        assert_eq!(store.load_library().books.len(), 1);

        // Importing the same file twice must not duplicate it.
        let again = import_file(&store, &archive, &mut library).unwrap();
        assert_eq!(again.id, book.id);
        assert_eq!(library.books.len(), 1);
    }

    #[test]
    fn progress_round_trips_per_book() {
        let dir = crate::testutil::scratch("progress");
        let store = Store::new(dir.join("data"));

        assert_eq!(store.load_progress("missing").percent, 0.0);

        let progress = crate::model::Progress {
            chapter: 3,
            offset: 1200,
            percent: 0.42,
            updated: 1,
        };
        store.save_progress("book", &progress).unwrap();

        let loaded = store.load_progress("book");
        assert_eq!(loaded.chapter, 3);
        assert_eq!(loaded.offset, 1200);
        assert!((loaded.percent - 0.42).abs() < 1e-9);
    }

    #[test]
    fn plain_text_is_split_into_parts() {
        let dir = crate::testutil::scratch("txt");
        let path = dir.join("long.txt");
        std::fs::write(&path, "word ".repeat(6000)).unwrap();

        let parsed = crate::formats::plain::import_txt(&path).unwrap();
        assert!(parsed.chapters.len() >= 2);
        assert!(parsed.chapters[0].html.starts_with("<p>"));
    }

    #[test]
    fn markdown_headings_become_chapters() {
        let dir = crate::testutil::scratch("md");
        let path = dir.join("notes.md");
        std::fs::write(
            &path,
            "# First\n\nSome *emphasis* and **weight**.\n\n## Second\n\n- one\n- two\n",
        )
        .unwrap();

        let parsed = crate::formats::plain::import_md(&path).unwrap();
        assert_eq!(parsed.title, "First");
        assert_eq!(parsed.chapters.len(), 2);
        assert!(parsed.chapters[0].html.contains("<em>emphasis</em>"));
        assert!(parsed.chapters[0].html.contains("<strong>weight</strong>"));
        assert!(parsed.chapters[1].html.contains("<li>one</li>"));
    }
}
