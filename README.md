# book-reader

A small, local, single-user ebook reader for Windows. Open a book, read it, close
it, and it opens on the same page next time. No accounts, no sync, no network.

- **Formats:** EPUB, FB2, plain text, Markdown
- **Stack:** Rust + Tauri v2 + WebView2, static HTML/CSS/vanilla JS
- **Data:** plain JSON files under `%APPDATA%\dev.bookreader.app\`

## Download

Grab the latest release:

[**book-reader-v0.1.0-windows-x64.exe**](https://github.com/lalithaar/book-reader/releases/download/v0.1.0/book-reader-v0.1.0-windows-x64.exe)
([releases page](https://github.com/lalithaar/book-reader/releases))

There is no installer. Download the `.exe`, copy it anywhere you like, and run
it. It needs the WebView2 runtime, which is preinstalled on Windows 10/11.

## Build and run

Requirements: Rust (MSVC toolchain), Visual Studio Build Tools with the Windows
SDK, and the WebView2 runtime (preinstalled on Windows 10/11).

```powershell
cd src-tauri
cargo run --release
```

The binary is `src-tauri\target\release\book-reader.exe`. It is self-contained:
copy it anywhere and it runs (the UI is embedded in the executable).

## Using it

Open the app and press <kbd>O</kbd> or the **Add books** button, pick one or more
files, and reading starts immediately. Files can also be dragged onto the window.

| Key | Action |
| --- | --- |
| <kbd>→</kbd> <kbd>↓</kbd> <kbd>Space</kbd> <kbd>PageDown</kbd> | Next page |
| <kbd>←</kbd> <kbd>↑</kbd> <kbd>PageUp</kbd> | Previous page |
| <kbd>Shift</kbd>+<kbd>Space</kbd> | Previous page |
| <kbd>Home</kbd> / <kbd>End</kbd> | Start / end of chapter |
| <kbd>T</kbd> | Table of contents |
| <kbd>B</kbd> | Bookmark this page |
| <kbd>Shift</kbd>+<kbd>B</kbd> | Bookmark list |
| <kbd>F</kbd> or <kbd>Ctrl</kbd>+<kbd>F</kbd> | Find in book |
| <kbd>S</kbd> | Typography |
| <kbd>F11</kbd> | Full screen |
| <kbd>Esc</kbd> | Close panel, or back to the library |

Clicking near the left edge of the page turns back and near the right edge turns
forward; the bookmark list lives behind the toolbar button. Typography, theme and
column count are remembered per app, and the reading position is remembered per
book.

## How it works

```
ui/                      static front end (no build step)
  index.html             library + reader markup
  app.css                themes, typography, chrome
  app.js                 pagination, panels, keyboard, persistence
src-tauri/
  src/lib.rs             app setup, window restore, book:// asset protocol
  src/commands.rs        the whole IPC surface
  src/formats/epub.rs    EPUB: spine, TOC, cover, inline images
  src/formats/fb2.rs     FB2: sections, inline base64 images
  src/formats/plain.rs   TXT chunking, Markdown headings
  src/html.rs            tolerant HTML tokenizer and sanitizer
  src/store.rs           JSON on disk, atomic writes
  src/import.rs          one-file import pipeline
harness/                 browser test harness (development only, not shipped)
```

On import a book is unpacked once and each chapter is pre-rendered to a
sanitised HTML fragment under `books/<id>/ch/<n>.html`. The reader only ever
injects those fragments, so book CSS and scripts can never affect the app, and
page turns do no parsing. Relative image links are rewritten to a custom
`book://` URL scheme that serves files from the app data directory with a
path-traversal guard.

Pagination is CSS multi-column: the chapter is laid out into columns exactly one
viewport wide, and a page turn is a `scrollLeft` jump. The reading position is
stored as `(chapter, character offset)`, which stays correct when the font size,
column count or window size changes.

## Data

```
%APPDATA%\dev.bookreader.app\
  library.json          books, covers, progress summary
  settings.json         theme, typography, last window geometry
  books\<id>\
    index.json          chapter list and TOC
    progress.json       chapter + character offset
    bookmarks.json
    ch\<n>.html         pre-rendered chapter fragments
    content\...         images referenced by chapters
    source.epub         the original file
```

Everything is written with `tmp` + rename, so a crash mid-write cannot corrupt
the library. Deleting a book removes its directory; deleting the whole folder
resets the app.

## Development

Rust unit tests (import, sanitizer, link rewriting, pagination offsets, store
round-trips):

```powershell
cd src-tauri
cargo test --lib
```

The front end has no build step and no dependencies. To iterate on it without
rebuilding the desktop shell, serve the repository root and open the harness,
which mocks the Tauri IPC:

```powershell
python -m http.server 8712
# then open http://localhost:8712/harness/index.html
```

`harness/index.html` is generated from `ui/index.html`; rerun
`powershell -File harness/sync.ps1` after changing the markup.

## Limits

PDF, MOBI/AZW3 and comics are not supported. EPUB support covers the common
cases: EPUB 2/3 navigation, NCX fallback, covers, and images; fixed-layout books
render as reflowable text. There is no syncing and no library-wide search.
