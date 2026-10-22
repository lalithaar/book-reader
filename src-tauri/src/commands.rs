use std::fs;

use tauri::State;

use crate::html;
use crate::import;
use crate::model::{
    now, Bookmark, BookView, ChapterPayload, Hit, OpenPayload, Progress,
};
use crate::store::{self, Store};
use crate::AppState;

#[derive(serde::Serialize)]
pub struct ImportReport {
    books: Vec<BookView>,
    errors: Vec<String>,
}

#[tauri::command]
pub fn list_books(state: State<'_, AppState>) -> Vec<BookView> {
    let store: &Store = &state.store;
    let library = state.library.lock().unwrap();
    let mut views: Vec<BookView> = library.books.iter().map(|b| store::view(store, b)).collect();
    views.sort_by(|a, b| b.opened.cmp(&a.opened).then(b.added.cmp(&a.added)));
    views
}

#[tauri::command]
pub async fn pick_books() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Open a book")
            .add_filter("Books", &["epub", "fb2", "txt", "md", "markdown"])
            .pick_files()
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<String>>()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub async fn import_books(state: State<'_, AppState>, paths: Vec<String>) -> Result<ImportReport, String> {
    let mut report = ImportReport {
        books: Vec::new(),
        errors: Vec::new(),
    };

    for path in paths {
        let path = std::path::PathBuf::from(path);
        if !path.exists() || path.is_dir() {
            continue;
        }
        let mut library = state.library.lock().unwrap();
        match import::import_file(&state.store, &path, &mut library) {
            Ok(book) => report.books.push(store::view(&state.store, &book)),
            Err(error) => report.errors.push(format!(
                "{}: {}",
                path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                error
            )),
        }
    }

    if report.books.is_empty() && !report.errors.is_empty() {
        return Err(report.errors.join("\n"));
    }
    Ok(report)
}

#[tauri::command]
pub fn open_book(state: State<'_, AppState>, id: String) -> Result<OpenPayload, String> {
    let index = state
        .store
        .load_index(&id)
        .ok_or_else(|| "book is missing from the library".to_string())?;

    let mut library = state.library.lock().unwrap();
    let book = library
        .books
        .iter_mut()
        .find(|b| b.id == id)
        .ok_or_else(|| "book is missing from the library".to_string())?;
    book.opened = now();
    let snapshot = book.clone();
    let _ = state.store.save_library(&library);

    Ok(OpenPayload {
        book: store::view(&state.store, &snapshot),
        index,
        progress: state.store.load_progress(&id),
        bookmarks: state.store.load_bookmarks(&id),
    })
}

#[tauri::command]
pub fn get_chapter(state: State<'_, AppState>, id: String, index: i64) -> Result<ChapterPayload, String> {
    let book = state
        .store
        .load_index(&id)
        .ok_or_else(|| "book is missing".to_string())?;
    let meta = book
        .chapters
        .get(index.max(0) as usize)
        .ok_or_else(|| "chapter is out of range".to_string())?;

    let path = state.store.chapter_path(&id, index);
    let html = fs::read_to_string(&path).map_err(|e| format!("cannot read chapter: {}", e))?;

    Ok(ChapterPayload {
        html,
        title: meta.title.clone(),
        chars: meta.chars,
        before: meta.before,
    })
}

#[tauri::command]
pub fn save_progress(
    state: State<'_, AppState>,
    id: String,
    chapter: i64,
    offset: i64,
    percent: f64,
) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("unknown book".into());
    }
    let progress = Progress {
        chapter: chapter.max(0),
        offset: offset.max(0),
        percent: percent.clamp(0.0, 1.0),
        updated: now(),
    };
    state.store.save_progress(&id, &progress)
}

#[tauri::command]
pub fn add_bookmark(
    state: State<'_, AppState>,
    id: String,
    chapter: i64,
    offset: i64,
    percent: f64,
    label: String,
) -> Result<Vec<Bookmark>, String> {
    let mut marks = state.store.load_bookmarks(&id);
    marks.retain(|m| !(m.chapter == chapter && (m.offset - offset).abs() <= 40));
    marks.push(Bookmark {
        chapter,
        offset,
        percent,
        label,
        created: now(),
    });
    marks.sort_by_key(|m| (m.chapter, m.offset));
    state.store.save_bookmarks(&id, &marks)?;
    Ok(marks)
}

#[tauri::command]
pub fn remove_bookmark(state: State<'_, AppState>, id: String, index: usize) -> Result<Vec<Bookmark>, String> {
    let mut marks = state.store.load_bookmarks(&id);
    if index < marks.len() {
        marks.remove(index);
        state.store.save_bookmarks(&id, &marks)?;
    }
    Ok(marks)
}

#[tauri::command]
pub fn delete_book(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut library = state.library.lock().unwrap();
    library.books.retain(|b| b.id != id);
    state.store.save_library(&library)?;
    state.store.remove(&id)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> std::collections::HashMap<String, String> {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn set_setting(state: State<'_, AppState>, key: String, value: String) -> Result<(), String> {
    let snapshot = {
        let mut settings = state.settings.lock().unwrap();
        settings.insert(key, value);
        settings.clone()
    };
    store::write_json(&state.store.settings_path(), &snapshot)
}

#[tauri::command]
pub fn save_window(window: tauri::Window, state: State<'_, AppState>) -> Result<(), String> {
    // Read the real geometry instead of trusting numbers from the web view:
    // CSS pixels and physical pixels diverge on scaled displays, which would
    // otherwise shrink the window a little on every launch.
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let position = window.outer_position().map_err(|e| e.to_string())?;

    let snapshot = {
        let mut settings = state.settings.lock().unwrap();
        settings.insert("win_w".into(), size.width.to_string());
        settings.insert("win_h".into(), size.height.to_string());
        settings.insert("win_x".into(), position.x.to_string());
        settings.insert("win_y".into(), position.y.to_string());
        settings.clone()
    };
    store::write_json(&state.store.settings_path(), &snapshot)
}

#[tauri::command]
pub fn toggle_fullscreen(window: tauri::Window) -> bool {
    let full = window.is_fullscreen().unwrap_or(false);
    let _ = window.set_fullscreen(!full);
    !full
}

#[tauri::command]
pub fn search_book(state: State<'_, AppState>, id: String, query: String) -> Result<Vec<Hit>, String> {
    let needle = query.trim().to_lowercase();
    if needle.chars().count() < 2 {
        return Ok(Vec::new());
    }
    let index = state
        .store
        .load_index(&id)
        .ok_or_else(|| "book is missing".to_string())?;

    let mut hits = Vec::new();
    for (number, chapter) in index.chapters.iter().enumerate() {
        let path = state.store.chapter_path(&id, number as i64);
        let html = match fs::read_to_string(&path) {
            Ok(html) => html,
            Err(_) => continue,
        };
        let text = html::text_of(&html);
        let lowered = text.to_lowercase();
        if lowered.contains(&needle) {
            for offset in lowered.match_indices(&needle).map(|(b, _)| b).take(4) {
                let char_offset = text[..offset].chars().count() as i64;
                let start = text[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
                let end = text[offset..]
                    .find('\n')
                    .map(|i| offset + i)
                    .unwrap_or(text.len());
                let snippet = html::collapse(&text[start.min(end)..end]);
                let snippet = if snippet.chars().count() > 160 {
                    snippet.chars().take(160).collect::<String>() + "…"
                } else {
                    snippet
                };
                hits.push(Hit {
                    chapter: number as i64,
                    offset: char_offset,
                    percent: if index.total_chars > 0 {
                        (chapter.before + char_offset) as f64 / index.total_chars as f64
                    } else {
                        0.0
                    },
                    snippet,
                });
            }
        }
        if hits.len() >= 200 {
            break;
        }
    }
    Ok(hits)
}
