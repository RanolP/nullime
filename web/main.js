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
let preedit = null;
let engine = null;

function insert(text) {
  const add = Array.from(text);
  chars.splice(caret, 0, ...add);
  caret += add.length;
}

// Default editing action for a key the engine did not handle.
function applyDefault(key) {
  switch (key) {
    case KEY.Space: insert(' '); break;
    case KEY.Enter: insert('\n'); break;
    case KEY.Backspace: if (caret > 0) chars.splice(--caret, 1); break;
    case KEY.Left: caret = Math.max(0, caret - 1); break;
    case KEY.Right: caret = Math.min(chars.length, caret + 1); break;
  }
}

function render() {
  editor.replaceChildren();
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
  render();
}

editor.addEventListener('keydown', (e) => {
  if (!engine || BARE_MODIFIERS.has(e.code)) return; // bare modifier presses must not flush the composition
  const [key, ch, shift, modifier] = mapKey(e);
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

editor.addEventListener('mousedown', () => setTimeout(() => editor.focus(), 0));

document.getElementById('clear').addEventListener('click', () => {
  if (engine) engine.reset();
  chars = [];
  caret = 0;
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
