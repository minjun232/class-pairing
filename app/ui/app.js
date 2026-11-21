// 자리배치 화면 동작.
// 교사 조건과 관련된 내용은 화면 어디에도 나오지 않는다(백엔드도 보내지 않는다).
'use strict';

const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const $ = (s) => document.querySelector(s);
const $$ = (s) => Array.from(document.querySelectorAll(s));
const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
};

const S = {
  screen: 'class',
  classes: [],
  selected: null,
  view: null,        // ClassView
  classroom: null,   // ClassroomConfig
  layout: null,      // Layout
  // ④
  arranged: null,    // { layout, names }
  revealed: new Set(),
  confirmed: false,
  editing: false,
  picked: null,
  reqDirty: false,
  rosterDirty: false,
};

// ---------------------------------------------------------------- 공통
function show(screen) {
  S.screen = screen;
  for (const s of ['class', 'roster', 'room', 'req', 'seat']) $('#screen-' + s).hidden = s !== screen;
  $('#nav').hidden = screen === 'class';
  $$('#nav .tabs button').forEach((b) => b.classList.toggle('active', b.dataset.go === screen));
  // 준비(명단·교실)에서는 [자리 배치 시작], 진행(요청·배치)에서는 단계 표시
  const progress = screen === 'req' || screen === 'seat';
  $('#btn-start').hidden = progress;
  $('#steps').hidden = !progress;
  renderSteps();
  syncStart();
  requestAnimationFrame(fitAll);
}

// 진행 단계: 요청 → 배치 → 공개 → 확정
function renderSteps() {
  const step = S.screen === 'req' ? 0 : !S.arranged ? 1 : S.confirmed ? 3 : 2;
  $$('#steps li').forEach((li, i) => li.classList.toggle('on', i === step));
}

// 준비가 끝났는지 (명단 있음, 자리가 충분함). 부족하면 이유, 끝났으면 null
function notReady() {
  const v = S.view;
  if (!v) return '반을 먼저 골라 주세요';
  if (v.roster_error) return v.roster_error;
  if (!v.students.length) return '명단을 먼저 적어 주세요';
  const lack = v.students.length - S.layout.seats.length;
  if (lack > 0) return `자리가 ${lack}개 부족해요`;
  return null;
}

function syncStart() {
  const why = notReady();
  $('#btn-start').disabled = !!why;
  $('#btn-start').title = why || '';
  $('#btn-start').textContent = inProgress() ? '배치로 돌아가기' : '자리 배치 시작';
}

// 확정하지 않은 배치가 있으면 준비 화면에 다녀와도 그 배치로 돌아간다
const inProgress = () => !!S.arranged && !S.confirmed;

async function startArranging() {
  if (S.screen === 'roster' && S.rosterDirty && !(await saveRoster())) return;
  if (inProgress()) {
    show('seat');
    renderSeat();
    return;
  }
  await refreshView();
  const why = notReady();
  if (why) { toast(why); syncStart(); return; }
  resetSeat();
  S.prev = null;
  await go('req');
}

let toastTimer = null;
function toast(text, withFolder) {
  $('#toast-text').textContent = text;
  $('#toast-action').hidden = !withFolder;
  $('#toast').hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => ($('#toast').hidden = true), withFolder ? 8000 : 3000);
}

function ask(content, okText) {
  return new Promise((resolve) => {
    const box = $('#modal-text');
    box.replaceChildren();
    if (typeof content === 'string') box.textContent = content;
    else box.append(content);
    $('#modal-ok').textContent = okText || '확인';
    $('#modal').hidden = false;
    const done = (v) => {
      $('#modal').hidden = true;
      $('#modal-ok').onclick = $('#modal-cancel').onclick = null;
      resolve(v);
    };
    $('#modal-ok').onclick = () => done(true);
    $('#modal-cancel').onclick = () => done(false);
  });
}

// ---------------------------------------------------------------- 모델 상태 (작게 표시)
async function pollStatus() {
  try {
    const st = await invoke('status');
    let text = '';
    if (st.model === 'loading') text = '모델 불러오는 중…';
    else if (st.model === 'missing') text = '모델 파일 없음';
    else if (st.model === 'unsupported' || st.model === 'failed') text = '모델 사용 불가';
    else if (st.pending > 0) text = '읽는 중…';
    $('#model-status').textContent = text;
    $('#model-status-2').textContent = text;
    S.status = st;
    syncArrangeButtons();
  } catch (_) { /* 무시 */ }
}

// 조건·요청을 읽는 중이거나 배치하는 중에는 배치 버튼을 누를 수 없다
function syncArrangeButtons() {
  const off = S.arranging || (S.status?.pending ?? 0) > 0;
  ['#btn-arrange', '#btn-rearrange', '#btn-req-next'].forEach((b) => ($(b).disabled = off));
}

// ---------------------------------------------------------------- ① 반 선택
function renderClasses() {
  const list = $('#class-list');
  list.replaceChildren();
  if (!S.classes.length) list.append(el('p', 'empty', '아직 반이 없어요. 아래에서 반을 추가해 주세요.'));
  for (const c of S.classes) {
    const b = el('button', c === S.selected ? 'selected' : '', c);
    b.onclick = () => { S.selected = c; renderClasses(); };
    b.ondblclick = openSelected;
    list.append(b);
  }
  $('#btn-open-class').disabled = !S.selected;
}

async function boot() {
  const b = await invoke('boot');
  S.classes = b.classes;
  S.selected = b.last;
  renderClasses();
}

async function addClass() {
  const name = $('#new-class').value.trim();
  $('#class-msg').className = 'msg';
  try {
    const b = await invoke('add_class', { name });
    S.classes = b.classes;
    S.selected = b.last;
    $('#new-class').value = '';
    renderClasses();
    $('#class-msg').textContent = '';
    // 새 반은 바로 열어서 명단부터 입력한다
    await openSelected();
  } catch (e) {
    $('#class-msg').className = 'msg error';
    $('#class-msg').textContent = e;
  }
}

async function openSelected() {
  if (!S.selected) return;
  try {
    applyView(await invoke('open_class', { name: S.selected }));
    resetSeat();
    S.prev = null;
    $('#nav-classname').textContent = S.view.name;
    // 명단이 비어 있으면 명단 화면부터
    if (!S.view.students.length && !S.view.roster_error) {
      show('roster');
      renderRoster();
    } else {
      show('room');
      renderRoom();
    }
  } catch (e) {
    $('#class-msg').className = 'msg error';
    $('#class-msg').textContent = e;
  }
}

function applyView(v) {
  S.view = v;
  S.classroom = v.classroom;
  S.layout = v.layout;
  syncStart();
}

async function refreshView() {
  try { applyView(await invoke('refresh')); } catch (_) { /* 무시 */ }
}

// ---------------------------------------------------------------- ② 교실 설정
const GENDER_LABEL = { pair: '남녀 짝', group: '남녀 고르게' };

function renderRoom() {
  const c = S.classroom;
  $$('#preset button').forEach((b) => b.classList.toggle('on', b.dataset.v === c.preset));
  $$('#gender button').forEach((b) => b.classList.toggle('on', b.dataset.v === c.gender));
  $('#group-size-box').hidden = c.preset !== 'group';
  $('#group-size').textContent = c.group_size;
  $('#gender-box').hidden = c.preset === 'single';
  $('#gender-mixed').textContent = GENDER_LABEL[c.preset] || '';
  $('#rows').textContent = c.rows;
  $('#cols').textContent = c.cols;
  const n = S.view.students.length;
  const seats = S.layout.seats.length;
  $('#room-count').textContent = `자리 ${seats} · 학생 ${n}`;
  $('#btn-room-prev').hidden = !S.view.previous_date;
  $('#room-msg').textContent = S.view.roster_error || (n > seats ? `자리가 ${n - seats}개 부족해요` : '');
  drawGrid($('#room-grid'), S.layout, (cell) => {
    if (cell.kind === 'blank') cell.node.append(el('span', 'tag', '빈자리'));
    cell.node.onclick = () => cycleCell(cell.row, cell.col, cell.kind);
  });
  fitAll();
}

async function saveRoom() {
  try {
    S.layout = await invoke('set_classroom', { classroom: S.classroom });
  } catch (e) { $('#room-msg').textContent = e; }
  syncStart();
  renderRoom();
}

function cycleCell(row, col, kind) {
  const next = { seat: 'blank', blank: 'aisle', aisle: 'seat' }[kind];
  const edits = S.classroom.edits.filter((e) => !(e.row === row && e.col === col));
  // 모둠 인원이 홀수일 때 생기는 통로 칸도 다시 자리로 만들 수 있게, 바뀐 값을 그대로 기록
  edits.push({ row, col, kind: next });
  S.classroom.edits = edits;
  saveRoom();
}

// 프리셋을 바꾸면 반 인원에 맞는 줄·열로 시작한다 (이후 교사가 조절).
function fitDims() {
  const c = S.classroom;
  const n = S.view.students.length || 30;
  if (c.preset === 'group') {
    const groups = Math.max(1, Math.ceil(n / c.group_size));
    c.cols = Math.min(4, groups);
    c.rows = Math.ceil(groups / c.cols);
  } else {
    c.cols = 6;
    c.rows = Math.max(1, Math.ceil(n / 6));
  }
}

function setupRoomControls() {
  $$('#preset button').forEach((b) => (b.onclick = () => {
    if (S.classroom.preset === b.dataset.v) return;
    S.classroom.preset = b.dataset.v;
    S.classroom.edits = [];
    fitDims();
    saveRoom();
  }));
  $$('#gender button').forEach((b) => (b.onclick = () => { S.classroom.gender = b.dataset.v; saveRoom(); }));
  const stepper = (box, key, min, max) => $$(box + ' button').forEach((b) => (b.onclick = () => {
    const v = Math.min(max, Math.max(min, S.classroom[key] + Number(b.dataset.step)));
    if (v === S.classroom[key]) return;
    S.classroom[key] = v;
    // 칸 편집은 격자 좌표로 저장된다. 모둠은 모둠 하나가 2열 × ⌈인원/2⌉줄이므로 격자 크기로 바꿔 비교한다.
    const c = S.classroom;
    const maxRow = c.preset === 'group' ? c.rows * Math.ceil(c.group_size / 2) : c.rows;
    const maxCol = c.preset === 'group' ? c.cols * 2 : c.cols;
    c.edits = c.edits.filter((e) => e.row <= maxRow && e.col <= maxCol);
    saveRoom();
  }));
  stepper('#rows-box', 'rows', 1, 20);
  stepper('#cols-box', 'cols', 1, 20);
  // 모둠: 최소 3명, 최대값은 반 인원 수
  $$('#group-size-box button').forEach((b) => (b.onclick = () => {
    const max = Math.max(3, S.view.students.length);
    const v = Math.min(max, Math.max(3, S.classroom.group_size + Number(b.dataset.step)));
    if (v === S.classroom.group_size) return;
    S.classroom.group_size = v;
    S.classroom.edits = [];
    fitDims();
    saveRoom();
  }));
}

// ---------------------------------------------------------------- 격자 그리기와 크기 맞추기
function drawGrid(grid, layout, decorate) {
  grid.replaceChildren();
  // 책상·모둠 사이에는 간격 트랙을 따로 둔다.
  const colTrack = [], rowTrack = [];
  const colTemplate = [], rowTemplate = [];
  for (let c = 1; c <= layout.cols; c++) {
    colTemplate.push('var(--cw)');
    colTrack[c] = colTemplate.length;
    if (layout.col_gap_after[c - 1]) colTemplate.push('var(--aisle)');
  }
  for (let r = 1; r <= layout.rows; r++) {
    rowTemplate.push('var(--ch)');
    rowTrack[r] = rowTemplate.length;
    if (layout.row_gap_after[r - 1]) rowTemplate.push('var(--aisle)');
  }
  grid.style.gridTemplateColumns = colTemplate.join(' ');
  grid.style.gridTemplateRows = rowTemplate.join(' ');
  grid.dataset.cols = layout.cols;
  grid.dataset.rows = layout.rows;
  grid.dataset.gaps = layout.col_gap_after.filter(Boolean).length;
  grid.dataset.rgaps = layout.row_gap_after.filter(Boolean).length;
  grid.dataset.preset = layout.preset;
  const seatIndex = new Map(layout.seats.map((s, i) => [s.row + ',' + s.col, i]));
  for (let r = 1; r <= layout.rows; r++) {
    for (let c = 1; c <= layout.cols; c++) {
      const kind = layout.cells[(r - 1) * layout.cols + (c - 1)];
      const node = el('div', 'cell ' + kind);
      node.style.gridColumn = colTrack[c];
      node.style.gridRow = rowTrack[r];
      if (kind !== 'aisle') node.append(el('div', 'desk'));
      grid.append(node);
      decorate({ node, row: r, col: c, kind, seat: seatIndex.get(r + ',' + c) });
    }
  }
}

// 남은 공간에 맞춰 칸 크기와 글자 크기를 정한다 (FHD/4K 자동 대응).
function fit(grid) {
  if (!grid || grid.offsetParent === null) return;
  const cols = +grid.dataset.cols, rows = +grid.dataset.rows;
  const gaps = +grid.dataset.gaps, rgaps = +grid.dataset.rgaps;
  if (!cols || !rows) return;
  const box = grid.parentElement.getBoundingClientRect();
  const other = Array.from(grid.parentElement.children)
    .filter((c) => c !== grid).reduce((h, c) => h + c.getBoundingClientRect().height, 0);
  const availW = box.width - 8;
  const availH = box.height - other - 16;
  // 짝·모둠은 책상 안 간격을 좁게, 책상 사이(통로)는 넓게
  const tight = grid.dataset.preset !== 'single';
  const gap = Math.max(4, Math.min(availW, availH) * (tight ? 0.006 : 0.012));
  const aisle = Math.max(10, Math.min(availW, availH) * 0.035);
  const tracksX = cols + gaps, tracksY = rows + rgaps;
  // 칸 비율 가로:세로 = 1.6:1
  const cwByW = (availW - gap * (tracksX - 1) - aisle * gaps) / cols;
  const chByH = (availH - gap * (tracksY - 1) - aisle * rgaps) / rows;
  const cw = Math.max(40, Math.min(cwByW, chByH * 1.6, 360));
  const ch = Math.max(28, Math.min(cw / 1.6, chByH));
  const longest = Math.max(3, ...Array.from(grid.querySelectorAll('.back')).map((b) => [...b.textContent].length));
  const name = Math.max(12, Math.min(ch * 0.42, (cw * 0.92) / Math.min(longest, 7)));
  const root = grid.closest('.screen');
  root.style.setProperty('--cw', cw + 'px');
  root.style.setProperty('--ch', ch + 'px');
  root.style.setProperty('--gap', gap + 'px');
  root.style.setProperty('--aisle', aisle + 'px');
  root.style.setProperty('--name', name + 'px');
}

function fitAll() {
  fit($('#room-grid'));
  fit($('#seat-grid'));
}
window.addEventListener('resize', fitAll);

// ---------------------------------------------------------------- ③ 학생 요청
function renderRequests() {
  const list = $('#req-list');
  list.replaceChildren();
  $('#req-msg').textContent = S.view.roster_error || '';
  if (!S.view.students.length && !S.view.roster_error) {
    list.append(el('p', 'req-empty', '명단.xlsx에 학생 이름을 적으면 여기에 나와요.'));
  }
  for (const st of S.view.students) {
    const row = el('div', 'req-row');
    row.append(el('span', 'who', st.name));
    const input = el('input');
    input.type = 'text';
    input.value = st.request || '';
    input.dataset.name = st.name;
    input.oninput = () => { S.reqDirty = true; $('#req-saved').textContent = ''; };
    input.onkeydown = (e) => { if (e.key === 'Enter') saveRequests(); };
    row.append(input);
    list.append(row);
  }
  $('#req-saved').textContent = '';
}

async function saveRequests() {
  const requests = {};
  $$('#req-list input').forEach((i) => (requests[i.dataset.name] = i.value));
  await invoke('save_requests', { requests });
  S.view.students.forEach((s) => (s.request = requests[s.name] || ''));
  S.reqDirty = false;
  $('#req-saved').textContent = '저장했어요';
}

// ---------------------------------------------------------------- ④ 자리 배치와 공개
function resetSeat() {
  S.arranged = null;
  S.revealed = new Set();
  S.confirmed = false;
  S.editing = false;
  S.picked = null;
}

function allRevealed() {
  return S.arranged && S.arranged.names.every((n, i) => !n || S.revealed.has(i));
}

function renderSeat() {
  const grid = $('#seat-grid');
  const a = S.arranged;
  // 배치 전이고 "현재 배치 보기"를 눌렀으면 확정된 배치를 모두 공개한 채 보여 준다
  const p = !a && S.prev;
  const layout = a ? a.layout : p ? p.layout : S.layout;
  $('#seat-caption').hidden = !p;
  if (p) $('#seat-caption').textContent = `현재 배치 · ${p.date}`;
  grid.classList.toggle('editing', S.editing);
  drawGrid(grid, layout, (cell) => {
    if (cell.seat == null) return;
    cell.node.replaceChildren();
    const name = a ? a.names[cell.seat] : p ? p.names[cell.seat] : null;
    const card = el('div', 'card');
    if (p) card.classList.add('revealed', ...(name ? [] : ['empty']));
    else if (!a) card.classList.add('empty', 'revealed');
    else if (!name) card.classList.add('empty');
    if (a && (S.revealed.has(cell.seat) || !name && allRevealed())) card.classList.add('revealed');
    if (S.picked === cell.seat) card.classList.add('picked');
    card.dataset.seat = cell.seat;
    const flip = el('div', 'flip');
    flip.append(el('div', 'face front', name ? '?' : ''), el('div', 'face back', name || ''));
    card.append(flip);
    card.onclick = () => onSeatClick(cell.seat);
    cell.node.append(card);
  });
  renderNames();
  renderSeatButtons();
  renderSteps();
  fitAll();
}

function renderNames() {
  const list = $('#name-list');
  const a = S.arranged;
  list.hidden = !a || S.confirmed || allRevealed();
  list.replaceChildren();
  if (!a) return;
  const entries = a.names.map((n, i) => [n, i]).filter(([n]) => n)
    .sort((x, y) => x[0].localeCompare(y[0], 'ko'));
  for (const [n, i] of entries) {
    const b = el('button', S.revealed.has(i) ? 'done' : '', n);
    b.onclick = () => reveal(i);
    list.append(b);
  }
}

function renderSeatButtons() {
  const a = !!S.arranged;
  const all = allRevealed();
  const prevMode = !a && !!S.prev;
  $('#btn-arrange').hidden = a;
  $('#btn-arrange').textContent = prevMode ? '새로 배치' : '자리 배치';
  $('#btn-show-prev').hidden = a || prevMode || !S.view.previous_date;
  $('#btn-reveal-all').hidden = !a || all;
  $('#btn-rearrange').hidden = !a || S.confirmed;
  $('#btn-edit').hidden = !a || !all || S.confirmed;
  $('#btn-edit').textContent = S.editing ? '편집 끝' : '편집';
  $('#btn-word').hidden = !(prevMode || (a && all));
}

function reveal(i) {
  if (S.revealed.has(i)) return;
  S.revealed.add(i);
  const card = $(`#seat-grid .card[data-seat="${i}"]`);
  if (card) {
    card.classList.add('revealed', 'just');
    setTimeout(() => card.classList.remove('just'), 1500);
  }
  renderNames();
  renderSeatButtons();
  if (allRevealed()) renderSeat();
}

function onSeatClick(i) {
  if (!S.arranged) return;
  if (!S.editing) { reveal(i); return; }
  // 편집: 두 자리를 차례로 눌러 맞바꾼다 (조건 검사 없음)
  if (S.picked == null) { S.picked = i; renderSeat(); return; }
  if (S.picked !== i) {
    const names = S.arranged.names;
    [names[S.picked], names[i]] = [names[i], names[S.picked]];
  }
  S.picked = null;
  renderSeat();
}

async function arrange() {
  // "현재 배치 보기"에서 [새로 배치]를 누르면 요청부터 다시 시작한다
  if (!S.arranged && S.prev) return startArranging();
  if (S.reqDirty) await saveRequests();
  if (S.screen !== 'seat') {
    show('seat');
    renderSeat();
  }
  S.arranging = true;
  syncArrangeButtons();
  $('#seat-msg').className = 'msg';
  try {
    for (;;) {
      try {
        const a = await invoke('arrange');
        resetSeat();
        S.prev = null;
        S.arranged = a;
        $('#seat-msg').textContent = '';
        renderSeat();
        return;
      } catch (e) {
        if (e !== 'wait') throw e;
        // 해석이 끝날 때까지 기다린다 (어떤 조건인지는 표시하지 않는다)
        $('#seat-msg').textContent = '준비하는 중이에요…';
        await new Promise((r) => setTimeout(r, 600));
      }
    }
  } catch (e) {
    $('#seat-msg').className = 'msg error';
    $('#seat-msg').textContent = e;
  } finally {
    S.arranging = false;
    syncArrangeButtons();
  }
}

async function showPrevious() {
  try {
    S.prev = await invoke('previous_view');
    resetSeat();
    show('seat');
    renderSeat();
  } catch (e) { toast(String(e)); }
}

async function saveWord() {
  try {
    let saved;
    if (!S.arranged && S.prev) {
      saved = await invoke('save_previous_docx');
    } else if (!S.confirmed) {
      const ok = await ask('저장하면 자리가 확정되어\n더 이상 수정할 수 없어요.', '확정·저장');
      if (!ok) return;
      saved = await invoke('confirm', { names: S.arranged.names });
      S.confirmed = true;
      S.editing = false;
      S.picked = null;
      renderSeat();
    } else {
      saved = await invoke('save_docx');
    }
    toast(`저장했어요: 반/${saved}`, true);
  } catch (e) {
    $('#seat-msg').className = 'msg error';
    $('#seat-msg').textContent = e;
  }
}

// ---------------------------------------------------------------- 종료
async function tryQuit() {
  if (S.rosterDirty && !(await saveRoster())) return;
  if (S.reqDirty) await saveRequests();
  const u = await invoke('unsaved');
  if (u.requests || u.unconfirmed) {
    const box = el('div');
    box.append(el('div', null, '종료하면 아래 내용이 저장되지 않아요.'));
    const ul = el('ul');
    if (u.requests) ul.append(el('li', null, '입력한 학생 요청'));
    if (u.unconfirmed) ul.append(el('li', null, '확정되지 않은 자리 배치'));
    box.append(ul);
    if (!(await ask(box, '종료'))) return;
  }
  await invoke('quit');
}

// ---------------------------------------------------------------- 명단 입력
const lines = (t) => t.split(/\r?\n/).map((s) => s.trim()).filter(Boolean);

function countRoster() {
  $('#count-m').textContent = lines($('#roster-m').value).length;
  $('#count-f').textContent = lines($('#roster-f').value).length;
}

function renderRoster() {
  const st = S.view.students;
  $('#roster-m').value = st.filter((s) => s.gender === 'male').map((s) => s.name).join('\n');
  $('#roster-f').value = st.filter((s) => s.gender === 'female').map((s) => s.name).join('\n');
  $('#roster-msg').className = S.view.roster_error ? 'msg error' : 'msg';
  $('#roster-msg').textContent = S.view.roster_error || '';
  S.rosterDirty = false;
  countRoster();
}

async function saveRoster() {
  // 엑셀에서 붙여 넣으면 한 줄에 "이름<탭>이름"이 올 수 있다 → 탭도 줄바꿈으로 본다
  const split = (t) => lines(t.replace(/\t/g, '\n'));
  try {
    applyView(await invoke('save_roster', { males: split($('#roster-m').value), females: split($('#roster-f').value) }));
    renderRoster();
    $('#roster-msg').textContent = `저장했어요 (${S.view.students.length}명)`;
    return true;
  } catch (e) {
    $('#roster-msg').className = 'msg error';
    $('#roster-msg').textContent = e;
    return false;
  }
}

// ---------------------------------------------------------------- 시작
async function go(screen) {
  if (S.screen === 'req' && S.reqDirty) await saveRequests();
  if (S.screen === 'roster' && S.rosterDirty && !(await saveRoster())) return;
  if (screen === 'class') {
    await boot();
    show('class');
    return;
  }
  await refreshView();
  show(screen);
  if (screen === 'roster') renderRoster();
  if (screen === 'room') renderRoom();
  if (screen === 'req') renderRequests();
  if (screen === 'seat') renderSeat();
}

function setup() {
  $$('[data-go]').forEach((b) => (b.onclick = () => go(b.dataset.go)));
  $$('[data-action="quit"]').forEach((b) => (b.onclick = tryQuit));
  $('#btn-add-class').onclick = addClass;
  $('#btn-save-roster').onclick = saveRoster;
  for (const id of ['#roster-m', '#roster-f']) {
    $(id).oninput = () => { S.rosterDirty = true; countRoster(); $('#roster-msg').textContent = ''; };
  }
  $('#new-class').onkeydown = (e) => { if (e.key === 'Enter') addClass(); };
  $('#btn-open-class').onclick = openSelected;
  $('#btn-save-req').onclick = saveRequests;
  $('#btn-req-next').onclick = arrange;
  $('#btn-start').onclick = startArranging;
  $('#btn-arrange').onclick = arrange;
  $('#btn-rearrange').onclick = arrange;
  $('#btn-reveal-all').onclick = () => {
    S.arranged.names.forEach((n, i) => n && S.revealed.add(i));
    renderSeat();
  };
  $('#btn-edit').onclick = () => { S.editing = !S.editing; S.picked = null; renderSeat(); };
  $('#btn-word').onclick = saveWord;
  $('#btn-show-prev').onclick = showPrevious;
  $('#btn-room-prev').onclick = showPrevious;
  $('#toast-action').onclick = () => invoke('open_folder');
  setupRoomControls();
  if (T.event) T.event.listen('close-requested', tryQuit);
  document.addEventListener('contextmenu', (e) => e.preventDefault());
  boot();
  pollStatus();
  setInterval(pollStatus, 1000);
}

setup();
