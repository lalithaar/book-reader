mod commands;
mod formats;
mod html;
mod import;
mod model;
mod store;

#[cfg(test)]
mod testutil;

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use tauri::http::{Request, Response};
use tauri::Manager;

/// Asset URLs look like `http://book.localhost/<book id>/content/...` on
/// Windows, where Tauri serves custom schemes over the `*.localhost` origin.
pub fn asset_base(id: &str) -> String {
    format!("http://book.localhost/{}", id)
}

pub struct AppState {
    pub store: store::Store,
    pub library: Mutex<model::Library>,
    pub settings: Mutex<HashMap<String, String>>,
}

pub fn run() {
    tauri::Builder::default()
        .register_uri_scheme_protocol("book", |ctx, request| serve(ctx.app_handle(), &request))
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data)?;
            let store = store::Store::new(data);

            let mut library = store.load_library();
            store.prune(&mut library);

            let settings: HashMap<String, String> =
                store::read_json(&store.settings_path()).unwrap_or_default();

            if let Some(window) = app.get_webview_window("main") {
                restore_window(&window, &settings);
            }

            app.manage(AppState {
                store,
                library: Mutex::new(library),
                settings: Mutex::new(settings),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_books,
            commands::pick_books,
            commands::import_books,
            commands::open_book,
            commands::get_chapter,
            commands::save_progress,
            commands::add_bookmark,
            commands::remove_bookmark,
            commands::delete_book,
            commands::get_settings,
            commands::set_setting,
            commands::save_window,
            commands::search_book,
            commands::toggle_fullscreen,
        ])
        .run(tauri::generate_context!())
        .expect("the reader failed to start");
}

fn restore_window(window: &tauri::WebviewWindow, settings: &HashMap<String, String>) {
    let number = |key: &str| settings.get(key).and_then(|v| v.parse::<i32>().ok());

    if let (Some(width), Some(height)) = (number("win_w"), number("win_h")) {
        // Physical pixels: only guard against nonsense values from a stale file.
        if width >= 320 && height >= 240 {
            let _ = window.set_size(tauri::PhysicalSize::new(width as u32, height as u32));
        }
    }
    if let (Some(x), Some(y)) = (number("win_x"), number("win_y")) {
        if x > -20000 && y > -20000 {
            let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
        }
    }
}

fn serve(app: &tauri::AppHandle, request: &Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let missing = || {
        Response::builder()
            .status(404)
            .header("Content-Type", "text/plain")
            .body(Cow::Borrowed(&b""[..]))
            .unwrap()
    };

    let raw = request.uri().path().to_string();
    let decoded = percent_encoding::percent_decode_str(&raw)
        .decode_utf8_lossy()
        .to_string();
    let relative = decoded.trim_start_matches('/').replace('\\', "/");

    if relative.is_empty() || relative.split('/').any(|segment| segment == "..") {
        return missing();
    }

    let state = app.state::<AppState>();
    let books = state.store.books_dir();
    let full = books.join(&relative);
    if !full.starts_with(&books) {
        return missing();
    }

    let bytes = match std::fs::read(&full) {
        Ok(bytes) => bytes,
        Err(_) => return missing(),
    };

    Response::builder()
        .status(200)
        .header("Content-Type", mime_for(&full))
        .header("Cache-Control", "max-age=31536000")
        .body(Cow::Owned(bytes))
        .unwrap()
}

fn mime_for(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "css" => "text/css",
        "xhtml" | "html" | "htm" => "text/html",
        _ => "application/octet-stream",
    }
}
