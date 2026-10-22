/* A quiet EPUB reader.
 *
 * Pagination model: chapter markup is injected into a single multi-column
 * block whose column width equals the page width, so the browser's own layout
 * engine does the page breaking. A page turn is one scrollLeft step. Reading
 * position is stored as a character offset into the chapter's text, which
 * survives font, margin and window changes.
 */

const TAURI = window.__TAURI__;
const invoke = (cmd, args) => {
  if (!TAURI) return Promise.reject(new Error('desktop bridge unavailable'));
  return TAURI.core.invoke(cmd, args);
};

const ASSET = 'http://book.localhost/';
const $ = (id) => document.getElementById(id);
const root = document.documentElement;
const body = document.body;

const DEFAULTS = {
  theme: 'sepia',
  font: 'serif',
  size: '19',
  lh: '1.6',
  measure: 'medium',
  justify: '1',
  hyph: '1',
  columns: '1',
  resume: '1',
};

const S = {
  books: [],
  book: null,
  index: null,
  chapter: 0,
  offset: 0,
  page: 0,
  pages: 1,
  before: 0,
  chars: 0,
  bookmarks: [],
  settings: { ...DEFAULTS },
  panel: null,
  token: 0,
};

/* ------------------------------------------------------------------ layout */

const artwork = () => document.getElementById('page');
const viewport = () => document.getElementById('viewport');
const pageWidth = () => Math.max(1, viewport().clientWidth);

function lineHeight() {
  const value = parseFloat(getComputedStyle(artwork()).lineHeight);
  const size = parseFloat(getComputedStyle(artwork()).fontSize) || 19;
  return isFinite(value) ? value : size * 1.6;
}

function applySettings() {
  const s = S.settings;
  const fonts = {
    serif: 'Georgia, "Iowan Old Style", "Times New Roman", serif',
    book: '"Palatino Linotype", Palatino, "Book Antiqua", Georgia, serif',
    sans: '"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif',
    mono: 'Consolas, "Cascadia Mono", monospace',
  };
  root.dataset.theme = s.theme;
  root.style.setProperty('--font', fonts[s.font] || fonts.serif);
  root.style.setProperty('--size', (parseInt(s.size, 10) || 19) + 'px');
  root.style.setProperty('--lh', s.lh || '1.6');
  root.style.setProperty('--align', s.justify === '1' ? 'justify' : 'left');
  root.style.setProperty('--hyph', s.hyph === '1' ? 'auto' : 'manual');
  body.dataset.columns = s.columns === '2' ? '2' : '1';
}

function applyFrame() {
  const size = parseInt(S.settings.size, 10) || 19;
  const ems = { narrow: 33, medium: 39, wide: 45 }[S.settings.measure] || 39;
  const measure = S.settings.columns === '2' ? ems * 2 + 4 : ems;
  const w = window.innerWidth;
  const h = window.innerHeight;
  const padX = Math.max(24, Math.round((w - measure * size) / 2));
  const padY = Math.round(Math.max(22, Math.min(78, h * 0.07)));
  root.style.setProperty('--pad-x', padX + 'px');
  root.style.setProperty('--pad-y', padY + 'px');
  root.style.setProperty('--page-h', h - padY * 2 + 'px');

  // `column-width` only accepts lengths, so the page width is computed here.
  const two = S.settings.columns === '2';
  const gap = two ? 24 : 0;
  const available = viewport().clientWidth || Math.max(120, w - padX * 2);
  const column = two ? Math.max(140, Math.round(available / 2 - gap)) : available;
  root.style.setProperty('--colw', column + 'px');
  root.style.setProperty('--colgap', gap + 'px');
}

function measurePages() {
  const page = artwork();
  page.scrollLeft = 0;
  if (viewport().clientWidth < 2) return;
  S.pages = Math.max(1, Math.ceil((page.scrollWidth - 1) / pageWidth()));
  S.page = Math.max(0, Math.min(S.page, S.pages - 1));
}

function scrollToPage() {
  artwork().scrollLeft = S.page * pageWidth();
  S.offset = caretOffset();
  updateHud();
}

function updateHud() {
  const total = (S.index && S.index.total_chars) || 1;
  const at = S.before + (S.offset || 0);
  const percent = Math.max(0, Math.min(100, Math.round((at / total) * 100)));
  $('page-label').textContent = `${S.page + 1} / ${S.pages}  ·  ${percent}%`;
  $('progress').style.width = percent + '%';
}

/* ------------------------------------------------------- offset bookkeeping */

function textWalker(from) {
  return document.createTreeWalker(from || artwork(), NodeFilter.SHOW_TEXT);
}

function firstTextFrom(node) {
  if (node.nodeType === Node.TEXT_NODE) return node;
  const walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
  return walker.nextNode();
}

function charIndex(target, within) {
  const walker = textWalker();
  let total = 0;
  let node;
  while ((node = walker.nextNode())) {
    if (node === target) return total + Math.min(within, node.data.length);
    total += node.data.length;
  }
  return total;
}

function positionOf(offset) {
  const walker = textWalker();
  let total = 0;
  let node;
  let last = null;
  while ((node = walker.nextNode())) {
    const length = node.data.length;
    if (offset < total + length) return { node, offset: Math.max(0, offset - total) };
    total += length;
    last = node;
  }
  if (last) return { node: last, offset: Math.max(0, last.data.length - 1) };
  return null;
}

/** Character offset of the first visible character on the current page. */
/** Caret at viewport coordinates, normalised to a text node inside the page. */
function caretAt(x, y) {
  let container = null;
  let offset = 0;

  if (document.caretRangeFromPoint) {
    const range = document.caretRangeFromPoint(x, y);
    if (range) {
      container = range.startContainer;
      offset = range.startOffset;
    }
  } else if (document.caretPositionFromPoint) {
    const point = document.caretPositionFromPoint(x, y);
    if (point) {
      container = point.offsetNode;
      offset = point.offset;
    }
  }

  if (!container) return null;
  if (container.nodeType !== Node.TEXT_NODE) {
    const next = firstTextFrom(container);
    if (!next) return null;
    container = next;
    offset = 0;
  }
  if (!artwork().contains(container)) return null;
  return { container, offset };
}

/**
 * Character offset of the first character on the page.
 *
 * Probing one fixed point is not enough: when a column starts with a heading
 * margin or an image, the point can fall in blank space and the browser answers
 * with the nearest caret in the *next* column, which silently skips a page of
 * text. So find the first real line box on the page and probe just inside it.
 */
function caretOffset() {
  const box = artwork().getBoundingClientRect();
  if (box.width < 2 || box.height < 2) return S.offset || 0;

  const walker = textWalker();
  let total = 0;
  let node;
  while ((node = walker.nextNode())) {
    const length = node.data.length;
    if (length) {
      const range = document.createRange();
      range.selectNodeContents(node);
      const rects = range.getClientRects();
      let first = null;
      for (let i = 0; i < rects.length; i += 1) {
        const rect = rects[i];
        if (rect.width < 1 || rect.height < 1) continue;
        if (rect.left < box.left - 1 || rect.left >= box.right - 1) continue;
        if (rect.top < box.top - 1 || rect.top >= box.bottom - 1) continue;
        if (!first || rect.top < first.top - 0.5
          || (Math.abs(rect.top - first.top) <= 0.5 && rect.left < first.left)) {
          first = rect;
        }
      }
      if (first) {
        const hit = caretAt(first.left + 2, first.top + first.height / 2);
        if (hit) return charIndex(hit.container, hit.offset);
        return total;
      }
    }
    total += length;
  }
  return S.offset || 0;
}

/** Move to the page holding `offset`. */
function seek(offset) {
  const page = artwork();
  const anchor = Math.max(0, offset || 0);
  // Keep the precise anchor rather than the page start: repeated restyles must
  // converge on the same text, not walk backwards a page at a time.
  S.offset = anchor;
  page.scrollLeft = 0;

  const position = positionOf(anchor);
  if (!position) {
    S.page = 0;
    page.scrollLeft = 0;
    updateHud();
    return;
  }

  // A minimised window reports a zero-width viewport; keep the anchor and let
  // the resize handler re-seat the page once the window is back.
  if (viewport().clientWidth < 2) {
    updateHud();
    return;
  }

  let left = 0;
  try {
    const range = document.createRange();
    range.setStart(position.node, position.offset);
    range.setEnd(position.node, Math.min(position.offset + 1, position.node.data.length));
    const rects = range.getClientRects();
    const rect = rects.length ? rects[0] : range.getBoundingClientRect();
    if (rect && (rect.width > 0 || rect.height > 0)) {
      left = rect.left - page.getBoundingClientRect().left;
    }
  } catch (error) {
    left = 0;
  }

  if (!isFinite(left)) left = 0;
  S.page = Math.max(0, Math.min(S.pages - 1, Math.floor(left / pageWidth())));
  page.scrollLeft = S.page * pageWidth();
  updateHud();
}

/* --------------------------------------------------------------- navigation */

async function showChapter(number, offset) {
  if (!S.index) return;
  const target = Math.max(0, Math.min(S.index.chapters.length - 1, number));
  const token = ++S.token;

  const data = await invoke('get_chapter', { id: S.book.id, index: target });
  if (token !== S.token) return;

  S.chapter = target;
  S.before = data.before;
  S.chars = data.chars;
  $('chapter-title').textContent = data.title || '';

  const page = artwork();
  page.innerHTML = data.html;
  page.scrollLeft = 0;
  S.page = 0;
  measurePages();
  seek(Math.max(0, Math.min(offset || 0, data.chars - 1)));
  scheduleSave();
}

function nextPage() {
  if (!S.book) return;
  if (S.page < S.pages - 1) {
    S.page += 1;
    scrollToPage();
    scheduleSave();
    wake();
  } else if (S.chapter < S.index.chapters.length - 1) {
    showChapter(S.chapter + 1, 0);
  } else {
    toast('End of book');
  }
}

function previousPage() {
  if (!S.book) return;
  if (S.page > 0) {
    S.page -= 1;
    scrollToPage();
    scheduleSave();
    wake();
  } else if (S.chapter > 0) {
    showChapter(S.chapter - 1, Number.MAX_SAFE_INTEGER);
  }
}

function follow(href) {
  if (!href) return;
  if (href.startsWith('#ch:')) {
    const chapter = parseInt(href.slice(4), 10);
    if (!isNaN(chapter)) showChapter(chapter, 0);
    return;
  }
  if (href.startsWith('#')) {
    let id = href.slice(1);
    try {
      id = decodeURIComponent(id);
    } catch (error) {
      /* keep raw */
    }
    const target = document.getElementById(id);
    if (!target) return;
    const page = artwork();
    page.scrollLeft = 0;
    const rect = target.getBoundingClientRect();
    const left = rect.left - page.getBoundingClientRect().left;
    S.page = Math.max(0, Math.min(S.pages - 1, Math.floor(left / pageWidth())));
    scrollToPage();
  }
}

/* ------------------------------------------------------------------ persist */

let saveTimer = 0;

function scheduleSave() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(saveNow, 600);
}

async function saveNow() {
  clearTimeout(saveTimer);
  if (!S.book || !S.index) return;

  const offset = Math.max(0, S.offset || 0);
  const total = S.index.total_chars || 1;
  const percent = Math.max(0, Math.min(1, (S.before + offset) / total));

  try {
    await invoke('save_progress', { id: S.book.id, chapter: S.chapter, offset, percent });
  } catch (error) {
    /* a failed write must never interrupt reading */
  }

  const entry = S.books.find((book) => book.id === S.book.id);
  if (entry) {
    entry.percent = percent;
    entry.chapter = S.chapter;
    entry.offset = offset;
  }
}

/* ------------------------------------------------------------------ library */

function assetUrl(id, relative) {
  return ASSET + id + '/' + relative;
}

async function refreshLibrary() {
  try {
    S.books = await invoke('list_books');
  } catch (error) {
    S.books = [];
  }
  renderLibrary();
}

function renderLibrary() {
  const query = $('filter').value.trim().toLowerCase();
  const grid = $('grid');
  grid.textContent = '';

  const list = S.books.filter((book) =>
    !query ? true : (book.title + ' ' + book.author).toLowerCase().includes(query)
  );

  $('empty').hidden = S.books.length > 0;

  for (const book of list) grid.appendChild(bookCard(book));
}

function bookCard(book) {
  const card = document.createElement('div');
  card.className = 'card';
  card.tabIndex = 0;
  card.setAttribute('role', 'button');

  const cover = document.createElement('div');
  cover.className = 'cover';

  const fallback = document.createElement('div');
  fallback.className = 'fallback';
  fallback.textContent = book.title;
  cover.appendChild(fallback);

  if (book.cover) {
    const image = document.createElement('img');
    image.alt = '';
    image.loading = 'lazy';
    image.src = assetUrl(book.id, book.cover);
    image.addEventListener('error', () => image.remove());
    cover.appendChild(image);
  }

  const remove = document.createElement('button');
  remove.className = 'remove';
  remove.textContent = '×';
  remove.title = 'Remove from library';
  remove.addEventListener('click', (event) => {
    event.stopPropagation();
    deleteBook(book);
  });
  cover.appendChild(remove);

  const title = document.createElement('div');
  title.className = 'title';
  title.textContent = book.title;

  const author = document.createElement('div');
  author.className = 'author';
  author.textContent = book.author;

  card.append(cover, title, author);

  if (book.percent > 0) {
    const track = document.createElement('div');
    track.className = 'track';
    const fill = document.createElement('i');
    fill.style.width = Math.round(book.percent * 100) + '%';
    track.appendChild(fill);
    card.appendChild(track);

    const done = document.createElement('div');
    done.className = 'done';
    done.textContent = book.finished ? 'Finished' : Math.round(book.percent * 100) + '%';
    card.appendChild(done);
  }

  const open = () => openBook(book.id);
  card.addEventListener('click', open);
  card.addEventListener('keydown', (event) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      open();
    }
  });

  return card;
}

async function openBook(id) {
  closePanel();
  try {
    const payload = await invoke('open_book', { id });
    S.book = payload.book;
    S.index = payload.index;
    S.bookmarks = payload.bookmarks || [];
    S.offset = payload.progress.offset || 0;

    $('book-title').textContent = payload.book.title;
    $('library').hidden = true;
    $('reader').hidden = false;
    body.classList.remove('idle');
    applyFrame();
    wake();

    await showChapter(payload.progress.chapter || 0, payload.progress.offset || 0);
  } catch (error) {
    toast(String(error));
  }
}

async function backToLibrary() {
  await saveNow();
  S.book = null;
  S.index = null;
  artwork().textContent = '';
  $('reader').hidden = true;
  $('library').hidden = false;
  closePanel();
  await refreshLibrary();
  $('filter').focus();
}

async function deleteBook(book) {
  if (!window.confirm(`Remove “${book.title}” from the library?`)) return;
  try {
    await invoke('delete_book', { id: book.id });
    if (S.book && S.book.id === book.id) {
      S.book = null;
      S.index = null;
      $('reader').hidden = true;
      $('library').hidden = false;
    }
    await refreshLibrary();
  } catch (error) {
    toast(String(error));
  }
}

async function addBooks() {
  let paths = [];
  try {
    paths = await invoke('pick_books');
  } catch (error) {
    toast(String(error));
    return;
  }
  if (paths && paths.length) await importPaths(paths);
}

async function importPaths(paths) {
  if (!paths || !paths.length) return;
  try {
    const report = await invoke('import_books', { paths });
    if (report.errors && report.errors.length) toast(report.errors.join(' · '));
    await refreshLibrary();
    if (report.books.length === 1 && !S.book) await openBook(report.books[0].id);
  } catch (error) {
    toast(String(error));
  }
}

/* -------------------------------------------------------------------- chrome */

let idleTimer = 0;
let pointerY = 0;

function wake() {
  clearTimeout(idleTimer);
  body.classList.remove('idle');
  idleTimer = setTimeout(() => {
    if (S.panel || $('reader').hidden) return;
    if (window.getSelection() && String(window.getSelection()).length) return;
    if (pointerY < 88 || pointerY > window.innerHeight - 46) return;
    body.classList.add('idle');
  }, 2000);
}

window.addEventListener('mousemove', (event) => {
  pointerY = event.clientY;
  if (body.classList.contains('idle') || !idleTimer) wake();
});

let toastTimer = 0;
function toast(message) {
  const node = $('toast');
  node.textContent = message;
  node.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    node.hidden = true;
  }, 4200);
}

/* -------------------------------------------------------------------- panels */

function openPanel(kind) {
  S.panel = kind;
  const panel = $('panel');
  panel.hidden = false;
  body.classList.remove('idle');
  clearTimeout(idleTimer);

  const titles = { toc: 'Contents', marks: 'Bookmarks', find: 'Find', settings: 'Typography' };
  $('panel-title').textContent = titles[kind] || '';
  const container = $('panel-body');
  container.textContent = '';

  if (kind === 'toc') renderToc(container);
  else if (kind === 'marks') renderMarks(container);
  else if (kind === 'find') renderFind(container);
  else renderSettings(container);
}

function closePanel() {
  $('panel').hidden = true;
  S.panel = null;
  wake();
}

function renderToc(container) {
  const entries = (S.index && S.index.toc) || [];
  for (const entry of entries) {
    const item = document.createElement('button');
    item.className = 'item';
    item.style.paddingLeft = 18 + entry.depth * 16 + 'px';
    const label = document.createElement('span');
    label.className = 'label';
    label.textContent = entry.title;
    item.appendChild(label);
    item.addEventListener('click', () => {
      closePanel();
      showChapter(entry.chapter, 0);
    });
    container.appendChild(item);
  }
}

function renderMarks(container) {
  if (!S.bookmarks.length) {
    container.appendChild(note('Press B to bookmark the page you are on.'));
    return;
  }
  S.bookmarks.forEach((mark, index) => {
    const item = document.createElement('button');
    item.className = 'item';

    const label = document.createElement('span');
    label.className = 'label';
    label.textContent = mark.label || 'Bookmark';

    const percent = document.createElement('span');
    percent.className = 'pct';
    percent.textContent = Math.round(mark.percent * 100) + '%';

    const remove = document.createElement('span');
    remove.className = 'pct';
    remove.textContent = '×';
    remove.title = 'Remove';
    remove.addEventListener('click', async (event) => {
      event.stopPropagation();
      try {
        S.bookmarks = await invoke('remove_bookmark', { id: S.book.id, index });
      } catch (error) {
        toast(String(error));
      }
      closePanel();
      openPanel('marks');
    });

    item.append(label, percent, remove);
    item.addEventListener('click', () => {
      closePanel();
      showChapter(mark.chapter, mark.offset);
    });
    container.appendChild(item);
  });
}

function renderFind(container) {
  const box = document.createElement('div');
  box.className = 'search-box';
  const input = document.createElement('input');
  input.placeholder = 'Search this book';
  input.spellcheck = false;
  box.appendChild(input);
  container.appendChild(box);

  const results = document.createElement('div');
  container.appendChild(results);

  let timer = 0;
  let token = 0;

  const run = async () => {
    const query = input.value.trim();
    results.textContent = '';
    if (query.length < 2) return;
    const mine = ++token;
    let hits = [];
    try {
      hits = await invoke('search_book', { id: S.book.id, query });
    } catch (error) {
      hits = [];
    }
    if (mine !== token) return;
    if (!hits.length) {
      results.appendChild(note('No matches.'));
      return;
    }
    for (const hit of hits) {
      const item = document.createElement('button');
      item.className = 'item';
      const text = document.createElement('span');
      text.className = 'label snippet';
      text.textContent = hit.snippet;
      const percent = document.createElement('span');
      percent.className = 'pct';
      percent.textContent = Math.round(hit.percent * 100) + '%';
      item.append(text, percent);
      item.addEventListener('click', () => {
        closePanel();
        showChapter(hit.chapter, hit.offset);
      });
      results.appendChild(item);
    }
  };

  input.addEventListener('input', () => {
    clearTimeout(timer);
    timer = setTimeout(run, 220);
  });
  input.focus();
}

function note(text) {
  const node = document.createElement('div');
  node.className = 'row muted';
  node.style.justifyContent = 'flex-start';
  node.textContent = text;
  return node;
}

function renderSettings(container) {
  const s = S.settings;

  const themeRow = row('Theme');
  const swatches = document.createElement('div');
  swatches.className = 'swatches';
  for (const name of ['paper', 'sepia', 'night', 'black']) {
    const button = document.createElement('button');
    button.className = 'swatch-' + name;
    button.title = name;
    button.setAttribute('aria-pressed', String(s.theme === name));
    button.addEventListener('click', () => {
      setSetting('theme', name);
      for (const other of swatches.children) other.setAttribute('aria-pressed', 'false');
      button.setAttribute('aria-pressed', 'true');
    });
    swatches.appendChild(button);
  }
  themeRow.appendChild(swatches);
  container.appendChild(themeRow);

  container.appendChild(
    segmented('Typeface', 'font', [
      ['serif', 'Serif'],
      ['book', 'Book'],
      ['sans', 'Sans'],
      ['mono', 'Mono'],
    ])
  );

  container.appendChild(
    slider('Size', 'size', 14, 30, 1, (value) => value + ' px')
  );
  container.appendChild(
    slider('Line spacing', 'lh', 1.25, 2.1, 0.05, (value) => Number(value).toFixed(2))
  );

  container.appendChild(
    segmented('Measure', 'measure', [
      ['narrow', 'Narrow'],
      ['medium', 'Medium'],
      ['wide', 'Wide'],
    ])
  );

  container.appendChild(toggle('Justify', 'justify'));
  container.appendChild(toggle('Hyphenate', 'hyph'));
  container.appendChild(toggle('Two pages', 'columns', '2', '1'));
  container.appendChild(toggle('Reopen last book', 'resume'));
}

function row(label) {
  const node = document.createElement('div');
  node.className = 'row';
  const text = document.createElement('span');
  text.textContent = label;
  node.appendChild(text);
  return node;
}

function segmented(label, key, options) {
  const node = row(label);
  const group = document.createElement('div');
  group.className = 'seg';
  for (const [value, text] of options) {
    const button = document.createElement('button');
    button.textContent = text;
    button.setAttribute('aria-pressed', String(S.settings[key] === value));
    button.addEventListener('click', () => {
      setSetting(key, value);
      for (const other of group.children) other.setAttribute('aria-pressed', 'false');
      button.setAttribute('aria-pressed', 'true');
    });
    group.appendChild(button);
  }
  node.appendChild(group);
  return node;
}

function slider(label, key, min, max, step, format) {
  const node = row(label);
  const wrap = document.createElement('div');
  wrap.style.display = 'flex';
  wrap.style.alignItems = 'center';
  wrap.style.gap = '10px';

  const input = document.createElement('input');
  input.type = 'range';
  input.min = String(min);
  input.max = String(max);
  input.step = String(step);
  input.value = String(S.settings[key] || min);

  const value = document.createElement('span');
  value.className = 'pct';
  value.style.minWidth = '46px';
  value.textContent = format(input.value);

  input.addEventListener('input', () => {
    value.textContent = format(input.value);
    setSetting(key, input.value);
  });

  wrap.append(input, value);
  node.appendChild(wrap);
  return node;
}

function toggle(label, key, onValue = '1', offValue = '0') {
  const node = row(label);
  const button = document.createElement('button');
  button.className = 'btn';
  button.style.padding = '5px 14px';
  const sync = () => {
    const on = S.settings[key] === onValue;
    button.textContent = on ? 'On' : 'Off';
    button.setAttribute('aria-pressed', String(on));
    button.style.opacity = on ? '1' : '.6';
  };
  sync();
  button.addEventListener('click', () => {
    setSetting(key, S.settings[key] === onValue ? offValue : onValue);
    sync();
  });
  node.appendChild(button);
  return node;
}

function setSetting(key, value) {
  S.settings[key] = String(value);
  invoke('set_setting', { key, value: String(value) }).catch(() => {});
  restyle();
}

/* ------------------------------------------------------------------- restyle */

function restyle() {
  if (!S.book) {
    applySettings();
    applyFrame();
    return;
  }
  const anchor = S.offset || 0;
  applySettings();
  applyFrame();
  measurePages();
  seek(anchor);
  scheduleSave();
}

let resizeTimer = 0;
window.addEventListener('resize', () => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(() => {
    if (!S.book) {
      applyFrame();
      return;
    }
    const anchor = S.offset || 0;
    applyFrame();
    measurePages();
    seek(anchor);
  }, 120);
});

/* ----------------------------------------------------------------- keyboard */

document.addEventListener('keydown', (event) => {
  const target = event.target;
  const typing =
    target &&
    (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable);

  if (event.key === 'F11') {
    event.preventDefault();
    invoke('toggle_fullscreen').catch(() => {});
    return;
  }

  if (typing) {
    if (event.key === 'Escape') target.blur();
    return;
  }

  const reading = S.book && !$('reader').hidden;

  if (!reading) {
    if (event.key === 'o' || event.key === 'O') {
      event.preventDefault();
      addBooks();
    }
    if (event.key === '/' || (event.ctrlKey && event.key === 'f')) {
      event.preventDefault();
      $('filter').focus();
    }
    return;
  }

  if (event.key === 'Escape') {
    event.preventDefault();
    if (S.panel) closePanel();
    else backToLibrary();
    return;
  }

  if (event.key === 'ArrowRight' || event.key === 'ArrowDown' || event.key === 'PageDown' || event.key === ' ') {
    event.preventDefault();
    if (event.shiftKey) previousPage();
    else nextPage();
    return;
  }
  if (event.key === 'ArrowLeft' || event.key === 'ArrowUp' || event.key === 'PageUp') {
    event.preventDefault();
    previousPage();
    return;
  }
  if (event.key === 'Home') {
    event.preventDefault();
    showChapter(S.chapter, 0);
    return;
  }
  if (event.key === 'End') {
    event.preventDefault();
    showChapter(S.chapter, Number.MAX_SAFE_INTEGER);
    return;
  }
  if (event.key === 't' || event.key === 'T') {
    event.preventDefault();
    openPanel('toc');
    return;
  }
  if (event.key === 'b' || event.key === 'B') {
    event.preventDefault();
    if (event.shiftKey) openPanel('marks');
    else bookmarkHere();
    return;
  }
  if (event.key === 'f' || event.key === 'F' || (event.ctrlKey && event.key === 'f')) {
    event.preventDefault();
    openPanel('find');
    return;
  }
  if (event.key === 's' || event.key === 'S') {
    event.preventDefault();
    openPanel('settings');
    return;
  }
});

async function bookmarkHere() {
  if (!S.book) return;
  const total = S.index.total_chars || 1;
  const percent = Math.min(1, (S.before + (S.offset || 0)) / total);
  const text = String(window.getSelection() || '').trim();
  const label = text ? text.slice(0, 60) : `${Math.round(percent * 100)}% · page ${S.page + 1}`;
  try {
    S.bookmarks = await invoke('add_bookmark', {
      id: S.book.id,
      chapter: S.chapter,
      offset: S.offset || 0,
      percent,
      label,
    });
    toast('Bookmark added');
  } catch (error) {
    toast(String(error));
  }
}

/* ------------------------------------------------------------- pointer input */

artwork().addEventListener('click', (event) => {
  const link = event.target.closest && event.target.closest('a');
  if (link) {
    event.preventDefault();
    follow(link.getAttribute('href'));
    return;
  }
  if (String(window.getSelection() || '').length) return;

  const x = event.clientX / window.innerWidth;
  if (x < 0.16) previousPage();
  else if (x > 0.84) nextPage();
  else wake();
});

let wheelTotal = 0;
let wheelLock = 0;

viewport().addEventListener(
  'wheel',
  (event) => {
    event.preventDefault();
    const now = performance.now();
    if (now < wheelLock) return;
    const delta = Math.abs(event.deltaY) > Math.abs(event.deltaX) ? event.deltaY : event.deltaX;
    wheelTotal += delta;
    if (Math.abs(wheelTotal) < 55) return;
    wheelTotal > 0 ? nextPage() : previousPage();
    wheelTotal = 0;
    wheelLock = now + 130;
  },
  { passive: false }
);

document.addEventListener('click', (event) => {
  const action = event.target.closest && event.target.closest('[data-act]');
  if (!action) return;
  switch (action.dataset.act) {
    case 'back':
      backToLibrary();
      break;
    case 'toc':
      openPanel('toc');
      break;
    case 'marks':
      openPanel('marks');
      break;
    case 'find':
      openPanel('find');
      break;
    case 'settings':
      openPanel('settings');
      break;
    case 'close-panel':
      closePanel();
      break;
  }
});

/* ---------------------------------------------------------------------- boot */

async function boot() {
  try {
    const saved = await invoke('get_settings');
    for (const [key, value] of Object.entries(saved || {})) {
      if (key in DEFAULTS) S.settings[key] = value;
    }
  } catch (error) {
    /* first run */
  }

  applySettings();
  applyFrame();

  $('import').addEventListener('click', addBooks);
  $('filter').addEventListener('input', renderLibrary);

  window.addEventListener('beforeunload', () => {
    // Never write a placeholder record: closing from the library must not
    // overwrite the position of the book we are no longer reading.
    if (S.book && S.index) {
      const offset = S.offset || 0;
      invoke('save_progress', {
        id: S.book.id,
        chapter: S.chapter,
        offset,
        percent: Math.min(1, (S.before + offset) / (S.index.total_chars || 1)),
      }).catch(() => {});
    }
    invoke('save_window').catch(() => {});
  });

  try {
    const handle = TAURI.window.getCurrentWindow();
    await handle.onDragDropEvent((event) => {
      const payload = event.payload;
      if (payload.type === 'over') {
        $('drop').hidden = false;
      } else if (payload.type === 'drop') {
        $('drop').hidden = true;
        importPaths(payload.paths);
      } else {
        $('drop').hidden = true;
      }
    });
  } catch (error) {
    /* drag and drop is a bonus */
  }

  // WebView2 does not reliably fire `beforeunload` when the native window is
  // closed, so remember the geometry as it changes instead.
  try {
    const handle = TAURI.window.getCurrentWindow();
    let geometryTimer = 0;
    const rememberWindow = () => {
      clearTimeout(geometryTimer);
      geometryTimer = setTimeout(() => {
        invoke('save_window').catch(() => {});
      }, 600);
    };
    await handle.onResized(rememberWindow);
    await handle.onMoved(rememberWindow);
  } catch (error) {
    /* remembering the window size is a convenience */
  }

  await refreshLibrary();

  if (S.settings.resume !== '0') {
    const last = S.books.filter((book) => book.opened > 0).sort((a, b) => b.opened - a.opened)[0];
    if (last) openBook(last.id);
  }
}

boot();
