/* Browser-side mock of the Tauri bridge, used only to exercise the reading
   engine outside the desktop shell. Not part of the app bundle. */

const PARAGRAPHS = [];
for (let i = 0; i < 40; i += 1) {
  PARAGRAPHS.push(
    `Paragraph ${i + 1}. ` + 'The quick brown fox jumps over the lazy dog. '.repeat(9)
  );
}

const CHAPTER_ONE = PARAGRAPHS.map((text) => `<p>${text}</p>`).join('');
const CHAPTER_TWO = '<h1>Two</h1><p>Second chapter text.</p>';
const CHARS_ONE = PARAGRAPHS.join('').length;
const CHARS_TWO = 26;

const book = {
  id: 'b1',
  title: 'Test Book',
  author: 'Ada Lovelace',
  format: 'epub',
  added: 1,
  opened: 2,
  chars: CHARS_ONE + CHARS_TWO,
  chapters: 2,
  cover: null,
  percent: 0.4,
  chapter: 0,
  offset: 6000,
  finished: false,
};

const index = {
  id: 'b1',
  title: 'Test Book',
  author: 'Ada Lovelace',
  language: 'en',
  format: 'epub',
  total_chars: CHARS_ONE + CHARS_TWO,
  chapters: [
    { title: 'One', chars: CHARS_ONE, before: 0 },
    { title: 'Two', chars: CHARS_TWO, before: CHARS_ONE },
  ],
  toc: [
    { title: 'Chapter One', chapter: 0, depth: 0 },
    { title: 'Chapter Two', chapter: 1, depth: 0 },
  ],
};

window.__calls = [];

function storedProgress() {
  try {
    const raw = window.localStorage.getItem('br.progress');
    if (raw) return JSON.parse(raw);
  } catch (error) {
    /* ignore */
  }
  return { chapter: 0, offset: 6000, percent: 0.4, updated: 1 };
}

window.__TAURI__ = {
  core: {
    invoke: async (command, args) => {
      window.__calls.push([command, args]);
      switch (command) {
        case 'get_settings':
          return {};
        case 'list_books':
          return [book];
        case 'open_book':
          return {
            book,
            index,
            progress: storedProgress(),
            bookmarks: [],
          };
        case 'get_chapter':
          return args.index === 0
            ? { html: CHAPTER_ONE, title: 'One', chars: CHARS_ONE, before: 0 }
            : { html: CHAPTER_TWO, title: 'Two', chars: CHARS_TWO, before: CHARS_ONE };
        case 'save_progress':
          try {
            window.localStorage.setItem('br.progress', JSON.stringify(args));
          } catch (error) {
            /* ignore */
          }
          return null;
        case 'set_setting':
        case 'save_window':
          return null;
        case 'search_book':
          return [];
        default:
          return null;
      }
    },
  },
  window: {
    getCurrentWindow: () => ({
      onDragDropEvent: async () => () => {},
    }),
  },
};
