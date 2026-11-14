// Cleanplate: a queue of images and videos cleaned in parallel within the device's cores and memory.
// Only the original File and the encoded result are kept per item (no decoded pixels), so long queues stay small.
import {
  BUDGET_MB, CORES, LOCAL_THREADS, SERVER_MAX_MB, cleanImageLocal, cleanVideoLocal, decodeImage, encodeImage, imageNeedMB,
  preferServer, serverClean, serverReady,
} from './engine.js';
import * as engine from './engine.js';
import { canZip, zip } from './zip.js';
import { initFeedback } from './feedback.js';

const $ = (id) => document.getElementById(id);
const IMAGE_TYPES = /^image\/(png|jpeg|webp)$/;
const VIDEO_EXT = /\.(mp4|mov|m4v)$/i;
// measured in the CLI: a second file in flight overlaps decoding and encoding with computation (+19%), a third
// adds little. The browser follows the same rule and splits the threads between them.
const MAX_JOBS = CORES >= 8 ? 2 : 1;

const items = [];
let nextId = 1;
let saveDir = null; // a folder chosen with "Save all to a folder": later results are written there as they finish
let skippedFiles = 0;

// ── adding files ──
const kindOf = (f) => (IMAGE_TYPES.test(f.type) ? 'image' : VIDEO_EXT.test(f.name) || /^video\/(mp4|quicktime)$/.test(f.type) ? 'video' : null);

function outName(it) {
  const stem = it.file.name.replace(/\.[^.]+$/, '');
  const ext = it.kind === 'video' ? 'mp4' : $('image-format').value;
  let name = `${stem}_clean.${ext}`;
  for (let k = 2; items.some((o) => o !== it && o.rel === it.rel && o.outName === name); k++) name = `${stem}_clean_${k}.${ext}`;
  return name;
}

function addFiles(list) {
  let added = 0;
  for (const { file, rel } of list) {
    const kind = kindOf(file);
    if (!kind || file.name.startsWith('.')) {
      skippedFiles++;
      continue;
    }
    const it = { id: nextId++, file, rel: rel || '', kind, status: 'queued', stage: 'Waiting', frac: 0 };
    items.push(it);
    it.outName = outName(it);
    renderNew(it);
    thumbnail(it);
    added++;
  }
  if (items.length) {
    $('dropzone').classList.add('hidden');
    $('queue-view').classList.remove('hidden');
  }
  summary();
  if (added) pump();
}

const fromInput = (input) => [...input.files].map((f) => ({ file: f, rel: (f.webkitRelativePath || '').split('/').slice(0, -1).join('/') }));

/** Files and folders dropped on the page, folders read recursively (keeping their relative paths). */
async function fromDrop(dt) {
  const entries = [...dt.items].map((i) => i.webkitGetAsEntry?.()).filter(Boolean);
  if (!entries.length) return [...dt.files].map((f) => ({ file: f, rel: '' }));
  const out = [];
  const walk = async (entry, path) => {
    if (entry.isFile) {
      const file = await new Promise((res, rej) => entry.file(res, rej));
      out.push({ file, rel: path });
    } else if (entry.isDirectory) {
      const reader = entry.createReader();
      const sub = path ? `${path}/${entry.name}` : entry.name;
      for (;;) {
        const batch = await new Promise((res, rej) => reader.readEntries(res, rej));
        if (!batch.length) break;
        for (const e of batch) await walk(e, sub);
      }
    }
  };
  for (const e of entries) await walk(e, '');
  return out;
}

// ── scheduler ──
function pump() {
  const running = items.filter((i) => i.status === 'running');
  for (const it of items) {
    if (it.status !== 'queued') continue;
    if (running.length >= MAX_JOBS) break;
    it.need = it.kind === 'video' ? engine.videoGuessMB(it.file.size) : imageNeedMB(...guessPixels(it));
    const used = running.reduce((a, r) => a + r.need, 0);
    if (running.length && used + it.need > BUDGET_MB) break; // keep the order, wait for memory
    running.push(it);
    run(it);
  }
}

/** Before decoding, a still's pixel count from its file size (generous, so the estimate errs on the safe side). */
const guessPixels = (it) => [Math.sqrt(it.file.size * (it.file.type === 'image/png' ? 0.8 : 8)), Math.sqrt(it.file.size * (it.file.type === 'image/png' ? 0.8 : 8))];

async function run(it) {
  it.status = 'running';
  it.t0 = performance.now();
  // split the cores between the files in flight, all of them when this is the last file
  const others = items.filter((i) => i !== it && (i.status === 'running' || i.status === 'queued')).length;
  const threads = others ? Math.max(2, Math.floor(LOCAL_THREADS / MAX_JOBS)) : LOCAL_THREADS;
  update(it);
  try {
    if (it.kind === 'image') await runImage(it, threads);
    else await runVideo(it, threads);
  } catch (err) {
    it.status = 'failed';
    it.error = err.message || String(err);
  }
  it.secs = (performance.now() - it.t0) / 1000;
  if (it.status === 'cleaned' && saveDir) await saveOne(it).catch((e) => (it.error = `saved in the browser only (${e.message})`));
  update(it);
  summary();
  pump();
}

const parseMark = (log, kind) => {
  const m = kind === 'video' ? log.match(/detected (\S+) x([\d.]+) at \((\d+),(\d+)\)/) : log.match(/detected (\S+) (\d+)px at \((\d+),(\d+)\)/);
  if (!m) return null;
  return kind === 'video' ? { name: m[1], x: +m[3] + 10 * +m[2], y: +m[4] + 5 * +m[2], size: 40 * +m[2] } : { name: m[1], size: +m[2], x: +m[3] + +m[2] / 2, y: +m[4] + +m[2] / 2 };
};

async function runImage(it, threads) {
  it.stage = 'Reading';
  update(it);
  const src = await decodeImage(it.file);
  [it.w, it.h] = [src.width, src.height];
  const need = imageNeedMB(src.width, src.height);
  const viaServer = engine.SERVER && it.file.size <= SERVER_MAX_MB * 2 ** 20 && (preferServer() || need > BUDGET_MB);
  if (!viaServer && need > BUDGET_MB)
    throw new Error(`needs about ${need} MB of memory, more than this browser can safely use (${BUDGET_MB} MB). Use the command-line tool.`);
  let out;
  if (viaServer) {
    it.where = `server, ${engine.SERVER.cores} cores`;
    const r = await serverClean(it.file, (f) => ((it.stage = f < 1 ? `Uploading ${Math.round(100 * f)}%` : 'Cleaning on the server'), update(it)));
    it.log = r.log;
    if (r.status !== 'cleaned') return void (it.status = 'refused');
    out = await decodeImage(r.blob);
  } else {
    it.stage = 'Cleaning';
    update(it);
    const r = await cleanImageLocal(it.file, src, threads);
    it.where = `${r.threads} threads in your browser`;
    it.log = r.log;
    if (r.status !== 'cleaned') return void (it.status = 'refused');
    out = r.out;
  }
  it.outBlob = await encodeImage(out, it.outName.split('.').pop());
  it.mark = parseMark(it.log, 'image');
  it.status = 'cleaned';
}

async function runVideo(it, threads) {
  const local = !preferServer() && 'VideoDecoder' in window && 'VideoEncoder' in window;
  const canServer = engine.SERVER && it.file.size <= SERVER_MAX_MB * 2 ** 20;
  const onServer = async () => {
    it.where = `server, ${engine.SERVER.cores} cores`;
    const r = await serverClean(it.file, (f) => ((it.stage = f < 1 ? `Uploading ${Math.round(100 * f)}%` : 'Cleaning on the server'), (it.frac = 0.3 * f), update(it)));
    return r;
  };
  let r;
  if (local) {
    try {
      r = await cleanVideoLocal(it.file, threads, (stage, frac) => {
        it.stage = stage;
        it.frac = frac;
        update(it);
      });
      it.where = `${r.threads} threads in your browser`;
    } catch (err) {
      if (!(err.tooBig && canServer)) throw err;
      r = await onServer();
    }
  } else if (canServer) {
    r = await onServer();
  } else {
    throw new Error('this browser cannot decode video here, use a recent Chrome or Edge, or the command-line tool');
  }
  it.log = r.log;
  if (r.status !== 'cleaned') return void (it.status = 'refused');
  it.outBlob = r.blob;
  it.mark = parseMark(it.log, 'video');
  it.status = 'cleaned';
}

// ── saving ──
/** The result blob, read back from the saved file when it was released from memory. */
const getOut = async (it) => it.outBlob || (it.outHandle && (await it.outHandle.getFile()));

async function dirFor(root, rel) {
  let d = root;
  for (const part of rel.split('/').filter(Boolean)) d = await d.getDirectoryHandle(part, { create: true });
  return d;
}

async function saveOne(it) {
  const fh = await (await dirFor(saveDir, it.rel)).getFileHandle(it.outName, { create: true });
  const w = await fh.createWritable();
  await w.write(await getOut(it));
  await w.close();
  it.outHandle = fh;
  it.saved = true;
  // big results live on disk now: drop the in-memory copy, it is read back from the file when needed
  if (it.outBlob && it.outBlob.size > 32 * 2 ** 20) it.outBlob = null;
}

async function saveAll() {
  try {
    saveDir = await window.showDirectoryPicker({ id: 'cleanplate', mode: 'readwrite' });
  } catch {
    return; // cancelled
  }
  for (const it of items.filter((i) => i.status === 'cleaned' && !i.saved)) {
    await saveOne(it).catch((e) => (it.error = `could not save (${e.message})`));
    update(it);
  }
  summary();
}

async function downloadZip() {
  const done = items.filter((i) => i.status === 'cleaned');
  if (!done.length) return;
  const files = [];
  for (const it of done) files.push({ name: it.rel ? `${it.rel}/${it.outName}` : it.outName, blob: await getOut(it) });
  if (!canZip(files)) return note('These results are over 4 GB together: save them to a folder instead, or download them one by one.');
  const btn = $('download-zip');
  btn.disabled = true;
  btn.textContent = 'Preparing the ZIP...';
  try {
    triggerDownload(await zip(files), `cleanplate-${new Date().toLocaleDateString('sv')}.zip`);
    done.forEach((it) => (it.downloaded = true));
  } finally {
    btn.disabled = false;
    btn.textContent = 'Download all (ZIP)';
  }
}

function triggerDownload(blob, name) {
  const a = Object.assign(document.createElement('a'), { href: URL.createObjectURL(blob), download: name });
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 60000);
}

// ── rendering ──
const STATUS = { queued: 'Waiting', running: 'Cleaning', cleaned: 'Cleaned', refused: 'No mark found', failed: 'Failed' };
const fmtMB = (b) => (b >= 2 ** 30 ? `${(b / 2 ** 30).toFixed(1)} GB` : `${(b / 2 ** 20).toFixed(1)} MB`);
const fmtSecs = (s) => (s >= 60 ? `${Math.floor(s / 60)}m ${String(Math.round(s % 60)).padStart(2, '0')}s` : `${s.toFixed(1)} s`);

function renderNew(it) {
  const li = document.createElement('li');
  li.className = 'q-item';
  li.dataset.id = it.id;
  li.innerHTML = `
    <div class="q-thumb ${it.kind}"></div>
    <div class="q-main">
      <div class="q-name"></div>
      <div class="q-meta"></div>
      <div class="q-bar"><span></span></div>
    </div>
    <span class="q-status"></span>
    <div class="q-actions">
      <button type="button" class="btn btn-secondary btn-xs" data-act="view">Compare</button>
      <button type="button" class="btn btn-secondary btn-xs" data-act="download">Download</button>
      <button type="button" class="btn btn-secondary btn-xs" data-act="report">Report</button>
      <button type="button" class="icon-btn" data-act="remove" title="Remove from the list" aria-label="Remove">×</button>
    </div>`;
  li.querySelector('.q-name').textContent = it.rel ? `${it.rel}/${it.file.name}` : it.file.name;
  $('queue').append(li);
  it.el = li;
  update(it);
}

let raf = 0;
const dirty = new Set();
/** Coalesce updates to one paint per frame (video progress arrives per frame). */
function update(it) {
  dirty.add(it);
  raf ||= requestAnimationFrame(() => {
    raf = 0;
    for (const i of dirty) paint(i);
    dirty.clear();
    summary();
  });
}

function paint(it) {
  const li = it.el;
  if (!li) return;
  li.dataset.status = it.status;
  const dims = it.w ? ` · ${it.w}×${it.h}` : '';
  const meta = [`${it.kind === 'video' ? 'Video' : 'Image'} · ${fmtMB(it.file.size)}${dims}`];
  if (it.status === 'running') meta.push(it.stage);
  if (it.status === 'cleaned') meta.push(`${fmtSecs(it.secs)} · ${it.where}${it.saved ? ' · saved' : ''}`);
  if (it.status === 'refused') meta.push('no mark found with confidence, nothing was changed');
  if (it.error) meta.push(it.error);
  li.querySelector('.q-meta').textContent = meta.join(' · ');
  li.querySelector('.q-status').textContent = STATUS[it.status];
  li.querySelector('.q-status').className = `q-status ${it.status}`;
  const frac = it.status === 'running' ? (it.kind === 'video' ? it.frac : 0.5) : it.status === 'queued' ? 0 : 1;
  li.querySelector('.q-bar span').style.width = `${(100 * frac).toFixed(1)}%`;
  li.querySelector('.q-bar').classList.toggle('indeterminate', it.status === 'running' && it.kind === 'image');
  // only the actions that make sense now
  li.querySelector('[data-act=view]').hidden = it.status !== 'cleaned';
  li.querySelector('[data-act=download]').hidden = it.status !== 'cleaned';
  li.querySelector('[data-act=report]').hidden = !['failed', 'refused'].includes(it.status);
  li.querySelector('[data-act=remove]').hidden = it.status === 'running';
  li.querySelector('.q-bar').hidden = !['queued', 'running'].includes(it.status);
  if (it.thumb) li.querySelector('.q-thumb').style.backgroundImage = `url(${it.thumb})`;
}

function summary() {
  const n = items.length;
  const count = (s) => items.filter((i) => i.status === s).length;
  const finished = count('cleaned') + count('refused') + count('failed');
  const running = items.filter((i) => i.status === 'running');
  const progress = n ? (finished + running.reduce((a, i) => a + (i.kind === 'video' ? i.frac : 0.5), 0)) / n : 0;
  $('overall-bar').style.width = `${(100 * progress).toFixed(1)}%`;
  const done = items.filter((i) => i.secs);
  const avg = done.length ? done.reduce((a, i) => a + i.secs, 0) / done.length : 0;
  const left = n - finished;
  const eta = avg && left ? ` · about ${fmtSecs((avg * left) / MAX_JOBS)} left` : '';
  const parts = [`${finished} of ${n} done`, `${count('cleaned')} cleaned`];
  if (count('refused')) parts.push(`${count('refused')} without a mark`);
  if (count('failed')) parts.push(`${count('failed')} failed`);
  $('summary').textContent = n ? parts.join(' · ') + eta : '';
  $('download-zip').disabled = !count('cleaned');
  note(
    [
      skippedFiles ? `${skippedFiles} file${skippedFiles > 1 ? 's were' : ' was'} skipped (not a PNG, JPG, WebP, MP4 or MOV).` : '',
      saveDir ? `New results are saved into “${saveDir.name}” as they finish.` : '',
    ]
      .filter(Boolean)
      .join(' '),
  );
}

const note = (t) => ($('queue-note').textContent = t);

async function thumbnail(it) {
  try {
    if (it.kind === 'image') {
      const b = await createImageBitmap(it.file, { resizeWidth: 112, resizeQuality: 'medium' });
      const c = new OffscreenCanvas(b.width, b.height);
      c.getContext('2d').drawImage(b, 0, 0);
      it.thumb = URL.createObjectURL(await c.convertToBlob({ type: 'image/jpeg', quality: 0.8 }));
    } else {
      const v = Object.assign(document.createElement('video'), { muted: true, preload: 'metadata', src: URL.createObjectURL(it.file) });
      await new Promise((res, rej) => ((v.onloadeddata = res), (v.onerror = rej)));
      [it.w, it.h] = [v.videoWidth, v.videoHeight];
      v.currentTime = Math.min(0.5, v.duration / 2 || 0);
      await new Promise((res) => (v.onseeked = res));
      const c = new OffscreenCanvas(112, Math.round((112 * v.videoHeight) / v.videoWidth) || 63);
      c.getContext('2d').drawImage(v, 0, 0, c.width, c.height);
      it.thumb = URL.createObjectURL(await c.convertToBlob({ type: 'image/jpeg', quality: 0.8 }));
      URL.revokeObjectURL(v.src);
    }
    update(it);
  } catch {
    /* no thumbnail: not decodable here, the job will say why */
  }
}

// ── viewer ──
let viewerUrls = [];
let vZoom = null;

async function openViewer(it) {
  viewerUrls.forEach((u) => URL.revokeObjectURL(u));
  viewerUrls = [];
  const out = await getOut(it);
  const outUrl = URL.createObjectURL(out);
  viewerUrls.push(outUrl);
  $('viewer-title').textContent = it.file.name;
  $('viewer-download').href = outUrl;
  $('viewer-download').download = it.outName;
  $('viewer-badge').textContent = `✓ ${it.mark ? it.mark.name.replace(/\.png$/, '') : 'mark'} removed · ${fmtSecs(it.secs)} · ${it.where}`;
  $('viewer-log').textContent = it.log || '';
  $('viewer-image').classList.toggle('hidden', it.kind !== 'image');
  $('viewer-video').classList.toggle('hidden', it.kind !== 'video');
  $('viewer').showModal();
  if (it.kind === 'image') {
    const [a, b] = await Promise.all([decodeImage(it.file), decodeImage(out)]);
    for (const [id, img] of [['cv-before', a], ['cv-after', b]]) {
      const cv = $(id);
      [cv.width, cv.height] = [img.width, img.height];
      cv.getContext('2d').putImageData(img, 0, 0);
    }
    const m = it.mark || { x: a.width - 64, y: a.height - 64, size: 96 };
    zoomStill(m.x, m.y, m.size * 2.4, a.width, a.height);
    setSplit(50);
  } else {
    const src = URL.createObjectURL(it.file);
    viewerUrls.push(src);
    $('v-before').src = src;
    $('v-after').src = outUrl;
    vZoom = it.mark;
  }
}

function zoomStill(cx, cy, side, w, h) {
  const s = Math.min(Math.round(side), w, h);
  const x = Math.max(0, Math.min(w - s, Math.round(cx - s / 2)));
  const y = Math.max(0, Math.min(h - s, Math.round(cy - s / 2)));
  for (const [from, to] of [['cv-before', 'zoom-before'], ['cv-after', 'zoom-after']]) {
    const g = $(to).getContext('2d');
    g.imageSmoothingEnabled = false;
    g.clearRect(0, 0, 220, 220);
    g.drawImage($(from), x, y, s, s, 0, 0, 220, 220);
  }
}

function setSplit(v) {
  $('compare-range').value = v;
  $('compare').style.setProperty('--pos', `${v}%`);
}

function zoomVideoFrame() {
  const a = $('v-before');
  if (!vZoom || !a.videoWidth || !$('viewer').open) return;
  const [W, H] = [a.videoWidth, a.videoHeight];
  const cw = Math.round(W * 0.12), ch = Math.round((cw * 9) / 16);
  const sx = Math.max(0, Math.min(W - cw, vZoom.x - cw / 2)), sy = Math.max(0, Math.min(H - ch, vZoom.y - ch / 2));
  for (const [v, c] of [['v-before', 'vz-before'], ['v-after', 'vz-after']]) {
    const g = $(c).getContext('2d');
    g.imageSmoothingEnabled = false;
    g.drawImage($(v), sx, sy, cw, ch, 0, 0, $(c).width, $(c).height);
  }
}
(function loop() {
  zoomVideoFrame();
  requestAnimationFrame(loop);
})();

// ── wiring ──
const feedback = initFeedback((kind, it) => ({
  page: location.origin + location.pathname,
  version: '2.0.0',
  browser: navigator.userAgent,
  cores: CORES,
  ram_gb: navigator.deviceMemory,
  local_threads: LOCAL_THREADS,
  files_in_flight: MAX_JOBS,
  memory_budget_mb: BUDGET_MB,
  isolated: self.crossOriginIsolated,
  server: engine.SERVER ? `${engine.SERVER.cores} cores` : null,
  queue: { total: items.length, cleaned: items.filter((i) => i.status === 'cleaned').length, failed: items.filter((i) => i.status === 'failed').length },
  ...(kind !== 'idea' && it
    ? { file: { kind: it.kind, type: it.file.type, size_mb: +(it.file.size / 2 ** 20).toFixed(2), width: it.w, height: it.h }, status: it.status, error: it.error, log: it.log }
    : {}),
}));

$('queue').addEventListener('click', async (e) => {
  const b = e.target.closest('button[data-act]');
  if (!b) return;
  const it = items.find((i) => i.id === +b.closest('.q-item').dataset.id);
  if (!it) return;
  if (b.dataset.act === 'view') openViewer(it);
  if (b.dataset.act === 'download') {
    triggerDownload(await getOut(it), it.outName);
    it.downloaded = true;
  }
  if (b.dataset.act === 'report') feedback.open(it.status === 'refused' ? 'refused' : 'bug', it);
  if (b.dataset.act === 'remove' && it.status !== 'running') {
    items.splice(items.indexOf(it), 1);
    it.el.remove();
    if (it.thumb) URL.revokeObjectURL(it.thumb);
    summary();
    pump();
  }
});

document.addEventListener('click', (e) => {
  const b = e.target.closest('[data-pick]');
  if (b) $(b.dataset.pick === 'folder' ? 'folder-input' : 'file-input').click();
});
$('dropzone').addEventListener('keydown', (e) => (e.key === 'Enter' || e.key === ' ') && (e.preventDefault(), $('file-input').click()));
for (const id of ['file-input', 'folder-input']) {
  $(id).addEventListener('change', (e) => {
    addFiles(fromInput(e.target));
    e.target.value = '';
  });
}
// the whole page is a drop target once files are queued
document.addEventListener('dragover', (e) => {
  e.preventDefault();
  $('workspace').classList.add('drag-over');
});
document.addEventListener('dragleave', (e) => !e.relatedTarget && $('workspace').classList.remove('drag-over'));
document.addEventListener('drop', async (e) => {
  e.preventDefault();
  $('workspace').classList.remove('drag-over');
  addFiles(await fromDrop(e.dataTransfer));
});
document.addEventListener('paste', (e) => addFiles([...e.clipboardData.files].map((f) => ({ file: f, rel: '' }))));

$('clear-done').addEventListener('click', () => {
  for (const it of items.filter((i) => ['cleaned', 'refused', 'failed'].includes(i.status))) {
    items.splice(items.indexOf(it), 1);
    it.el.remove();
    if (it.thumb) URL.revokeObjectURL(it.thumb);
  }
  skippedFiles = 0;
  if (!items.length) {
    $('queue-view').classList.add('hidden');
    $('dropzone').classList.remove('hidden');
  }
  summary();
});
$('download-zip').addEventListener('click', downloadZip);
if ('showDirectoryPicker' in window) {
  $('save-folder').classList.remove('hidden');
  $('save-folder').addEventListener('click', saveAll);
}
$('image-format').addEventListener('change', () => items.filter((i) => i.status === 'queued' && i.kind === 'image').forEach((i) => (i.outName = outName(i))));

$('compare-range').addEventListener('input', (e) => setSplit(e.target.value));
$('viewer-close').addEventListener('click', () => $('viewer').close());
$('viewer').addEventListener('close', () => {
  $('v-before').pause();
  $('v-after').pause();
  vZoom = null;
});
$('v-play').addEventListener('click', () => {
  const [a, b] = [$('v-before'), $('v-after')];
  a.currentTime = b.currentTime = 0;
  a.play();
  b.play();
});
for (const [a, b] of [['v-before', 'v-after'], ['v-after', 'v-before']]) {
  $(a).addEventListener('seeked', () => Math.abs($(a).currentTime - $(b).currentTime) > 0.05 && ($(b).currentTime = $(a).currentTime));
}

// leaving with work in progress or unsaved results loses them: ask first
window.addEventListener('beforeunload', (e) => {
  if (items.some((i) => i.status === 'running' || i.status === 'queued' || (i.status === 'cleaned' && !i.saved && !i.downloaded))) e.preventDefault();
});

serverReady.then(() => {
  if (preferServer()) {
    $('engine-pill').textContent = `Cleaned on a ${engine.SERVER.cores}-core server`;
    $('where-text').textContent = 'Files are cleaned on the server and deleted right after.';
  } else if (LOCAL_THREADS > 1) {
    $('engine-pill').textContent = `Runs on your device, ${LOCAL_THREADS} threads`;
  }
});
