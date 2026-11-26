import init, { Engine } from './pkg/nullime_wasm.js';

const KEY = { Other: 0, Char: 1, Space: 2, Enter: 3, Backspace: 4, Left: 5, Right: 6, Up: 7, Down: 8 };

// US-QWERTY: [unshifted, shifted]
const PUNCT = {
  Digit1: ['1', '!'], Digit2: ['2', '@'], Digit3: ['3', '#'], Digit4: ['4', '$'], Digit5: ['5', '%'],
  Digit6: ['6', '^'], Digit7: ['7', '&'], Digit8: ['8', '*'], Digit9: ['9', '('], Digit0: ['0', ')'],
  Minus: ['-', '_'], Equal: ['=', '+'], BracketLeft: ['[', '{'], BracketRight: [']', '}'],
  Backslash: ['\\', '|'], Semicolon: [';', ':'], Quote: ["'", '"'], Comma: [',', '<'],
  Period: ['.', '>'], Slash: ['/', '?'], Backquote: ['`', '~'],
};
const NAMED = {
  Space: KEY.Space, Enter: KEY.Enter, NumpadEnter: KEY.Enter, Backspace: KEY.Backspace,
  ArrowLeft: KEY.Left, ArrowRight: KEY.Right, ArrowUp: KEY.Up, ArrowDown: KEY.Down,
};

const BARE_MODIFIERS = new Set([
  'ShiftLeft', 'ShiftRight', 'ControlLeft', 'ControlRight', 'AltLeft', 'AltRight',
  'MetaLeft', 'MetaRight', 'OSLeft', 'OSRight', 'CapsLock',
]);

/** KeyboardEvent -> [key, ch, shift, modifier] */
function mapKey(e) {
  const modifier = e.ctrlKey || e.altKey || e.metaKey;
  const shift = e.shiftKey;
  if (/^Key[A-Z]$/.test(e.code)) return [KEY.Char, e.code.charCodeAt(3) + 32, shift, modifier];
  if (e.code in PUNCT) return [KEY.Char, PUNCT[e.code][shift ? 1 : 0].charCodeAt(0), shift, modifier];
  if (e.code in NAMED) return [NAMED[e.code], 0, shift, modifier];
  return [KEY.Other, 0, shift, modifier];
}

const editor = document.getElementById('editor');
const statusEl = document.getElementById('status');
const debugEl = document.getElementById('debug');

let chars = []; // committed text, one entry per code point
let caret = 0;  // index into chars
let anchor = null; // other end of the selection; null when nothing is selected
let preedit = null;
let engine = null;

/** [start, end) of the selection, or null when it is empty. */
function selRange() {
  if (anchor === null || anchor === caret) return null;
  return [Math.min(anchor, caret), Math.max(anchor, caret)];
}

/** Removes the selected text; returns whether there was any. */
function deleteSelection() {
  const r = selRange();
  anchor = null;
  if (!r) return false;
  chars.splice(r[0], r[1] - r[0]);
  caret = r[0];
  return true;
}

function insert(text) {
  deleteSelection();
  const add = Array.from(text);
  chars.splice(caret, 0, ...add);
  caret += add.length;
}

/** Deletes the selection if any, else the text between the caret and `target`. */
function deleteTo(target) {
  if (deleteSelection()) return;
  const [a, b] = [Math.min(caret, target), Math.max(caret, target)];
  chars.splice(a, b - a);
  caret = a;
}

// Word boundaries: runs of letters/digits (Hangul, Kana and Kanji are letters too),
// runs of other symbols, and whitespace between them.
const charClass = (c) => (/\s/u.test(c) ? 0 : /[\p{L}\p{M}\p{N}_]/u.test(c) ? 1 : 2);

function prevWord(i) {
  while (i > 0 && charClass(chars[i - 1]) === 0) i--;
  const c = i > 0 ? charClass(chars[i - 1]) : 0;
  while (i > 0 && charClass(chars[i - 1]) === c) i--;
  return i;
}

function nextWord(i) {
  const c = i < chars.length ? charClass(chars[i]) : 0;
  if (c !== 0) while (i < chars.length && charClass(chars[i]) === c) i++;
  while (i < chars.length && charClass(chars[i]) === 0) i++;
  return i;
}

// lastIndexOf with a negative start counts from the end, so position 0 needs its own case.
const lineStart = (i) => (i === 0 ? 0 : chars.lastIndexOf('\n', i - 1) + 1);
const lineEnd = (i) => { const j = chars.indexOf('\n', i); return j < 0 ? chars.length : j; };

const MOVES = {
  ArrowLeft: (ctrl) => (ctrl ? prevWord(caret) : Math.max(0, caret - 1)),
  ArrowRight: (ctrl) => (ctrl ? nextWord(caret) : Math.min(chars.length, caret + 1)),
  Home: (ctrl) => (ctrl ? 0 : lineStart(caret)),
  End: (ctrl) => (ctrl ? chars.length : lineEnd(caret)),
};

function moveCaret(code, ctrl, shift) {
  const r = selRange();
  if (!shift && !ctrl && r && (code === 'ArrowLeft' || code === 'ArrowRight')) {
    caret = code === 'ArrowLeft' ? r[0] : r[1]; // collapse the selection to its edge
    anchor = null;
    return;
  }
  const target = MOVES[code](ctrl);
  if (shift) anchor ??= caret;
  else anchor = null;
  caret = target;
}

/**
 * Editing shortcuts done on the JS side, after the composition is flushed.
 * Plain Left/Right stay with the engine (segment moves while composing).
 */
function editAction(code, ctrl, shift) {
  if (code in MOVES && (ctrl || shift || code === 'Home' || code === 'End')) {
    return () => moveCaret(code, ctrl, shift);
  }
  switch ((ctrl ? 'Ctrl+' : '') + code) {
    case 'Ctrl+KeyA': return () => { anchor = 0; caret = chars.length; };
    case 'Ctrl+Backspace': return () => deleteTo(prevWord(caret));
    case 'Ctrl+Delete': return () => deleteTo(nextWord(caret));
    case 'Delete': return () => deleteTo(Math.min(chars.length, caret + 1));
    case 'Ctrl+KeyC': case 'Ctrl+KeyX': return selRange() ? () => copySelection(code === 'KeyX') : null;
    case 'Ctrl+Enter': return () => addCase();
  }
  return null;
}

// Default editing action for a key the engine did not handle.
function applyDefault(key) {
  switch (key) {
    case KEY.Space: insert(' '); break;
    case KEY.Enter: insert('\n'); break;
    case KEY.Backspace: deleteTo(Math.max(0, caret - 1)); break;
    case KEY.Left: moveCaret('ArrowLeft', false, false); break;
    case KEY.Right: moveCaret('ArrowRight', false, false); break;
  }
}

function render() {
  editor.replaceChildren();
  const r = selRange();
  if (r) {
    // A selection never coexists with a preedit: every key that makes one flushes first.
    const sel = document.createElement('span');
    sel.className = 'selected';
    sel.textContent = chars.slice(r[0], r[1]).join('');
    const c = document.createElement('span');
    c.className = 'caret';
    c.setAttribute('aria-hidden', 'true');
    editor.append(document.createTextNode(chars.slice(0, r[0]).join('')));
    if (caret === r[0]) editor.append(c, sel);
    else editor.append(sel, c);
    editor.append(document.createTextNode(chars.slice(r[1]).join('')));
    return;
  }
  editor.append(document.createTextNode(chars.slice(0, caret).join('')));
  let focusedEl = null;
  if (preedit) {
    preedit.segments.forEach((seg, i) => {
      const el = document.createElement('span');
      el.className = `seg ${seg.lang}` + (i === preedit.focused ? ' focused' : '');
      el.textContent = seg.text;
      editor.append(el);
      if (i === preedit.focused) focusedEl = el;
    });
  }
  const c = document.createElement('span');
  c.className = 'caret';
  c.setAttribute('aria-hidden', 'true');
  editor.append(c, document.createTextNode(chars.slice(caret).join('')));
  if (preedit && focusedEl) showCandidates(preedit.segments[preedit.focused], focusedEl);
}

function showCandidates(seg, anchor) {
  const ul = document.createElement('ul');
  ul.id = 'cands';
  ul.setAttribute('role', 'listbox');
  ul.setAttribute('aria-label', '후보');
  seg.candidates.forEach((cand, i) => {
    const li = document.createElement('li');
    li.setAttribute('role', 'option');
    li.setAttribute('aria-selected', String(i === seg.selected));
    if (i === seg.selected) li.className = 'sel';
    const t = document.createElement('span');
    t.textContent = cand.text;
    const tag = document.createElement('span');
    tag.className = 'tag';
    tag.textContent = cand.lang;
    li.append(t, tag);
    ul.append(li);
  });
  editor.append(ul);
  const er = editor.getBoundingClientRect();
  const ar = anchor.getBoundingClientRect();
  const left = Math.max(0, Math.min(ar.left - er.left, editor.clientWidth - ul.offsetWidth));
  ul.style.left = `${left}px`;
  ul.style.top = `${ar.bottom - er.top + 2}px`;
}

function handle(res, key) {
  debugEl.textContent = JSON.stringify(res, null, 2);
  if (res.commit) insert(res.commit);
  if (!res.handled) applyDefault(key);
  preedit = res.preedit ?? null;
  // The last composition stays explained after it is committed, so it can be read and added as a case.
  if (preedit) {
    lastComposition = { explanation: engine.explain(), preedit };
    renderExplanation();
  }
  render();
}

// ---- Score breakdown (Engine::explain) ----

const explainEl = document.getElementById('explain');
let lastComposition = null; // { explanation, preedit } of the latest key that left a composition

const TERM_LABELS = {
  lm: '언어 모델', random_keys: '키 수×ln 26', word_list: '단어 목록', prior: '일본어 기본값',
  dictionary: '사전 증거', per_letter: '글자당', rank: '순위', unread: '해석 불가', score: '점수',
};
const num = (v) => (v > 0 ? '+' : '') + v.toFixed(2);

function cell(row, text, className) {
  const td = document.createElement('td');
  td.textContent = text;
  if (className) td.className = className;
  row.append(td);
  return td;
}

function renderExplanation() {
  const { explanation: x, preedit: p } = lastComposition;
  explainEl.replaceChildren();
  let total = 0;
  x.segments.forEach((seg, i) => {
    const selected = p.segments[i].selected;
    total += seg.candidates[selected].net;
    const head = document.createElement('p');
    head.className = 'seg-head';
    const keys = document.createElement('code');
    keys.textContent = seg.keys;
    head.append(`칸 ${i + 1} `, keys, seg.pinned ? ' (고정)' : '');
    const table = document.createElement('table');
    const hr = table.createTHead().insertRow();
    for (const h of ['후보', '언어', '점수', '구성', '전환', '경로 기여']) {
      const th = document.createElement('th');
      th.textContent = h;
      hr.append(th);
    }
    const body = table.createTBody();
    seg.candidates.forEach((c, j) => {
      const row = body.insertRow();
      if (j === selected) row.className = 'sel';
      cell(row, c.text);
      const lang = document.createElement('span');
      lang.className = `lang ${c.lang}`;
      lang.textContent = c.lang;
      cell(row, '').append(lang);
      cell(row, num(c.score), 'num');
      cell(row, c.terms.map((t) => `${TERM_LABELS[t.name] ?? t.name} ${num(t.value)}`).join(' · '), 'terms');
      cell(row, c.switch ? num(-c.switch) : '', 'num');
      cell(row, num(c.net), 'num');
    });
    explainEl.append(head, table);
  });
  const foot = document.createElement('p');
  foot.className = 'total';
  foot.textContent = `경로 점수 ${num(total)} = 고른 후보의 경로 기여 합. 경로 기여 = 점수 - 칸 벌점 ${x.segment_penalty} + 전환(한 단어 안에서 언어가 바뀌는 경계에만 붙는다). 키 입력열: ${x.keys}`;
  explainEl.append(foot);
}

// ---- Evaluation cases (tools/nullime-dict/mixed.tsv lines) ----

const casesEl = document.getElementById('cases');
const caseStatusEl = document.getElementById('case-status');
const CASES_KEY = 'nullime.cases';
casesEl.value = localStorage.getItem(CASES_KEY) ?? '';
casesEl.addEventListener('input', () => localStorage.setItem(CASES_KEY, casesEl.value));

function addCase() {
  if (!lastComposition) { caseStatusEl.textContent = '아직 조합한 것이 없다.'; return; }
  const { explanation, preedit: p } = lastComposition;
  const line = `${explanation.keys}\t${p.segments.map((s) => s.text).join('')}`;
  const v = casesEl.value;
  casesEl.value = v + (v && !v.endsWith('\n') ? '\n' : '') + line + '\n';
  localStorage.setItem(CASES_KEY, casesEl.value);
  caseStatusEl.textContent = `추가함: ${line.replace('\t', ' → ')}`;
}

// Keep the editor focused: a blur would commit the composition before the click.
document.getElementById('case-add').addEventListener('mousedown', (e) => e.preventDefault());
document.getElementById('case-add').addEventListener('click', addCase);
document.getElementById('case-copy').addEventListener('click', async () => {
  try {
    // navigator.clipboard exists only in secure contexts (https, localhost).
    await navigator.clipboard.writeText(casesEl.value);
    caseStatusEl.textContent = '복사함.';
  } catch {
    casesEl.select();
    caseStatusEl.textContent = '클립보드에 쓰지 못해 선택만 했다. Ctrl+C로 복사한다.';
  }
});
document.getElementById('case-clear').addEventListener('click', () => {
  casesEl.value = '';
  localStorage.removeItem(CASES_KEY);
  caseStatusEl.textContent = '비움.';
});

editor.addEventListener('keydown', (e) => {
  if (!engine || BARE_MODIFIERS.has(e.code)) return; // bare modifier presses must not flush the composition
  const action = !e.altKey && editAction(e.code, e.ctrlKey || e.metaKey, e.shiftKey);
  if (action) {
    e.preventDefault();
    // Other flushes the composition; commit it before editing so no preedit text is lost.
    handle(engine.key(KEY.Other, 0, e.shiftKey, e.ctrlKey || e.metaKey), KEY.Other);
    action();
    render();
    return;
  }
  const [key, ch, shift, modifier] = mapKey(e);
  if (!modifier && selRange()) {
    // No preedit exists while text is selected, so these replace the selection directly.
    if (key === KEY.Backspace) { e.preventDefault(); deleteSelection(); render(); return; }
    if (key === KEY.Char || key === KEY.Space || key === KEY.Enter) deleteSelection();
  }
  const res = engine.key(key, ch, shift, modifier);
  // Modifier combos and unmapped keys (Ctrl+R, F5, Tab, ...) keep the browser default;
  // the engine already flushed any composition for them.
  if (key !== KEY.Other && !modifier) e.preventDefault();
  handle(res, modifier || key === KEY.Other ? KEY.Other : key);
});

// Losing focus commits the composition.
editor.addEventListener('blur', () => {
  if (engine && preedit) handle(engine.reset(), KEY.Other);
});

// Number of code points in `chars` before DOM point (node, offset); preedit and candidate text do not count.
function indexOfPoint(node, offset) {
  const point = document.createRange();
  point.setStart(node, offset);
  const walker = document.createTreeWalker(editor, NodeFilter.SHOW_TEXT, {
    acceptNode: (t) => (t.parentElement.closest('.seg, #cands') ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT),
  });
  let n = 0;
  for (let t = walker.nextNode(); t; t = walker.nextNode()) {
    if (t === node) return n + Array.from(t.data.slice(0, offset)).length;
    if (point.comparePoint(t, 0) >= 0) break; // t starts at or after the point
    n += Array.from(t.data).length;
  }
  return n;
}

// chars index under the viewport point, clamped into the editor so a drag past its edge still selects.
function indexAt(x, y) {
  const r = editor.getBoundingClientRect();
  x = Math.min(Math.max(x, r.left + 1), r.right - 1);
  y = Math.min(Math.max(y, r.top + 1), r.bottom - 1);
  let node = null, offset = 0;
  if (document.caretPositionFromPoint) {
    const p = document.caretPositionFromPoint(x, y);
    if (p) [node, offset] = [p.offsetNode, p.offset];
  } else if (document.caretRangeFromPoint) {
    const p = document.caretRangeFromPoint(x, y);
    if (p) [node, offset] = [p.startContainer, p.startOffset];
  }
  if (!node || !editor.contains(node)) return y < r.top + r.height / 2 ? 0 : chars.length;
  return indexOfPoint(node, offset);
}

// [start, end) of the run of same-class characters around boundary i. Hit-testing snaps to the
// nearest boundary, so a click on the right half of a word's last letter lands after it: prefer
// the side that is a word (class 1), then a symbol over whitespace.
function wordAt(i) {
  if (chars.length === 0) return [0, 0];
  const left = i > 0 ? charClass(chars[i - 1]) : -1;
  const right = i < chars.length ? charClass(chars[i]) : -1;
  const c = right === 1 || left === -1 ? right : left === 1 || right === -1 || right === 0 ? left : right;
  let [a, b] = [i, i];
  while (a > 0 && charClass(chars[a - 1]) === c) a--;
  while (b < chars.length && charClass(chars[b]) === c) b++;
  return [a, b];
}

let dragging = false;

// The model owns the selection: the native one is suppressed so the two never disagree.
editor.addEventListener('mousedown', (e) => {
  if (e.button !== 0) return;
  e.preventDefault();
  editor.focus();
  if (e.target.closest('#cands')) return;
  // Commit the composition exactly as blur does, then hit-test the re-rendered text.
  if (engine && preedit) handle(engine.reset(), KEY.Other);
  const i = indexAt(e.clientX, e.clientY);
  if (e.detail === 2) {
    [anchor, caret] = wordAt(i);
  } else if (e.shiftKey) {
    anchor ??= caret;
    caret = i;
    dragging = true;
  } else {
    anchor = caret = i;
    dragging = true;
  }
  render();
});

document.addEventListener('mousemove', (e) => {
  if (!dragging) return;
  // A release outside the window never reaches mouseup; hovering must not keep extending.
  if (!(e.buttons & 1)) { dragging = false; return; }
  const i = indexAt(e.clientX, e.clientY);
  if (i === caret) return;
  caret = i;
  render();
});

document.addEventListener('mouseup', () => { dragging = false; });

// Ctrl+C/X write the clipboard directly: the editor holds no native selection, so the browser's
// own copy/cut may count as disabled and never fire the events below.
// Cut deletes only once the write succeeded, and only if the selection is still the one copied.
function copySelection(cut) {
  const r = selRange();
  const text = chars.slice(r[0], r[1]).join('');
  // navigator.clipboard exists only in secure contexts (https, localhost).
  const write = navigator.clipboard?.writeText(text) ?? Promise.reject(new Error('navigator.clipboard 없음'));
  write.then(() => {
    const now = selRange();
    if (cut && now && now[0] === r[0] && now[1] === r[1]) { deleteSelection(); render(); }
  }, (err) => {
    console.error(err);
    statusEl.textContent = `클립보드에 쓰지 못했다: ${err.message ?? err}`;
  });
}

// Menu-triggered copy/cut. The editor is not contenteditable, so clipboard events target <body>; take them while it has focus.
function onCopy(e, cut) {
  const r = selRange();
  if (document.activeElement !== editor || !r) return;
  e.preventDefault();
  e.clipboardData.setData('text/plain', chars.slice(r[0], r[1]).join(''));
  if (cut) { deleteSelection(); render(); }
}
document.addEventListener('copy', (e) => onCopy(e, false));
document.addEventListener('cut', (e) => onCopy(e, true));
document.addEventListener('paste', (e) => {
  if (document.activeElement !== editor || !engine) return;
  e.preventDefault();
  if (preedit) handle(engine.reset(), KEY.Other);
  insert((e.clipboardData?.getData('text/plain') ?? '').replace(/\r\n?/g, '\n'));
  render();
});

document.getElementById('clear').addEventListener('click', () => {
  if (engine) engine.reset();
  chars = [];
  caret = 0;
  anchor = null;
  preedit = null;
  debugEl.textContent = '(아직 없음)';
  render();
  editor.focus();
});

const dictStatusEl = document.getElementById('dict-status');
const dictFullBtn = document.getElementById('dict-full');
const dictFileInput = document.getElementById('dict-file');
const FULL_LABEL = '전체 사전';

// Status line describing the engine in use, without any load-failure note.
let currentStatus = '';

// Swapping engines drops the composition; commit it first so no text is lost.
function swapEngine(next, label) {
  if (engine && preedit) handle(engine.reset(), KEY.Other);
  engine?.free();
  engine = next;
  dictFullBtn.disabled = label === FULL_LABEL;
  const n = next.dictionaryEntries;
  currentStatus = n === undefined
    ? '사전 없음: 샘플 사전 사용'
    : `사전 로드됨: ${label} (${n.toLocaleString()}개 항목)`;
  dictStatusEl.textContent = currentStatus;
}

async function fetchDictionary(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return new Uint8Array(await res.arrayBuffer());
}

// Of the loads that succeed, the most recently started wins: a slow startup
// fetch must not replace a dictionary the user picked meanwhile, but a newer
// load that fails must not discard an older one that succeeds.
let latestStarted = 0;
let applied = 0; // sequence number of the load whose engine is in use

// Leaves the current engine in place when the dictionary cannot be loaded.
// Returns whether this load's dictionary is now in use.
async function loadDictionary(label, getBytes) {
  const load = ++latestStarted;
  dictStatusEl.textContent = `${label} 불러오는 중...`;
  try {
    const bytes = await getBytes();
    if (load < applied) return false;
    swapEngine(Engine.withDictionary(bytes), label);
    applied = load;
    return true;
  } catch (err) {
    console.error(err);
    if (load > applied) dictStatusEl.textContent = `${currentStatus} (${label} 로드 실패: ${err.message ?? err})`;
    return false;
  }
}

dictFullBtn.addEventListener('click', async () => {
  dictFullBtn.disabled = true;
  const ok = await loadDictionary(FULL_LABEL, () => fetchDictionary('assets/nullime-ja.dict'));
  if (!ok) dictFullBtn.disabled = false;
});

dictFileInput.addEventListener('change', async () => {
  const file = dictFileInput.files?.[0];
  if (file) await loadDictionary(file.name, async () => new Uint8Array(await file.arrayBuffer()));
});

render();
try {
  await init();
  swapEngine(new Engine(), '');
  statusEl.textContent = '준비됨. 입력 영역을 클릭하고 타자를 친다.';
} catch (err) {
  statusEl.textContent = `wasm 로드 실패: ${err}. web/README.md의 빌드 명령을 먼저 실행한다.`;
  dictStatusEl.textContent = '';
  console.error(err);
}
if (engine) await loadDictionary('작은 사전', () => fetchDictionary('assets/nullime-ja.small.dict'));
