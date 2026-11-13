// cleanplate web demo: decode → strip alpha → worker (wasm) → re-attach alpha → compare / download PNG.
const $ = (id) => document.getElementById(id);
const worker = new Worker('worker.js');
const views = ['dropzone', 'busy', 'refused', 'result'];
let cur = null; // { name, w, h, src: ImageData }
let timer = 0;

function show(v) {
  for (const id of views) $(id).classList.toggle('hidden', id !== v);
  $('error').classList.add('hidden');
}

function fail(msg) {
  clearInterval(timer);
  show('dropzone');
  $('error').textContent = msg;
  $('error').classList.remove('hidden');
}

async function handle(file) {
  if (!file) return;
  if (!/^image\/(png|jpeg|webp)$/.test(file.type)) return fail(`Unsupported file type: ${file.type || file.name}. Use PNG, JPG or WebP.`);
  let bmp;
  try {
    bmp = await createImageBitmap(file, { colorSpaceConversion: 'none', premultiplyAlpha: 'none' });
  } catch (err) {
    return fail(`Could not decode this image (${err.message}).`);
  }
  const { width: w, height: h } = bmp;
  const c = new OffscreenCanvas(w, h);
  const g = c.getContext('2d', { willReadFrequently: true });
  g.drawImage(bmp, 0, 0);
  const src = g.getImageData(0, 0, w, h);
  const rgb = new Uint8Array(w * h * 3);
  for (let i = 0, j = 0; i < src.data.length; i += 4, j += 3) {
    rgb[j] = src.data[i];
    rgb[j + 1] = src.data[i + 1];
    rgb[j + 2] = src.data[i + 2];
  }
  cur = { name: file.name.replace(/\.[^.]+$/, ''), w, h, src, t0: performance.now() };
  $('busy-name').textContent = file.name;
  show('busy');
  timer = setInterval(() => ($('busy-time').textContent = ((performance.now() - cur.t0) / 1000).toFixed(1)), 100);
  worker.postMessage({ rgb, w, h, lossy: file.type !== 'image/png' }, [rgb.buffer]);
}

worker.onmessage = ({ data }) => {
  clearInterval(timer);
  const secs = ((performance.now() - cur.t0) / 1000).toFixed(1);
  if (data.error) return fail(`cleanplate crashed on this image (${data.error}). Please report it.`);
  if (data.status !== 0) {
    $('refused-log').textContent = data.log;
    return show('refused');
  }
  const { w, h, src } = cur;
  const out = new ImageData(new Uint8ClampedArray(src.data), w, h);
  for (let i = 0, j = 0; i < out.data.length; i += 4, j += 3) {
    out.data[i] = data.rgb[j];
    out.data[i + 1] = data.rgb[j + 1];
    out.data[i + 2] = data.rgb[j + 2];
  }
  for (const [id, img] of [['cv-before', src], ['cv-after', out]]) {
    const cv = $(id);
    cv.width = w;
    cv.height = h;
    cv.getContext('2d').putImageData(img, 0, 0);
  }
  // "detected <name> <n>px at (x,y)" / "calibrated: … opacity 0.482 …"
  const m = data.log.match(/detected (\S+) (\d+)px at \((\d+),(\d+)\)/);
  const op = data.log.match(/opacity ([\d.]+)/);
  const size = m ? +m[2] : 96;
  const [mx, my] = m ? [+m[3], +m[4]] : [w - size * 2, h - size * 2];
  zoom(mx + size / 2, my + size / 2, size * 2.4);
  $('detect-badge').textContent =
    `✓ Removed ${m ? m[1].replace(/\.png$/, '') : 'mark'} · ${size}px` + (op ? ` · opacity ${(+op[1]).toFixed(2)}` : '') + ` · ${secs} s`;
  $('result-log').textContent = data.log;
  setSplit(50);
  show('result');
};
worker.onerror = (e) => fail(`Worker failed to start: ${e.message}`);

function zoom(cx, cy, side) {
  const s = Math.min(Math.round(side), cur.w, cur.h);
  const x = Math.max(0, Math.min(cur.w - s, Math.round(cx - s / 2)));
  const y = Math.max(0, Math.min(cur.h - s, Math.round(cy - s / 2)));
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
$('compare-range').addEventListener('input', (e) => setSplit(e.target.value));

$('btn-download').addEventListener('click', () =>
  $('cv-after').toBlob((b) => {
    const a = Object.assign(document.createElement('a'), { href: URL.createObjectURL(b), download: `${cur.name}_clean.png` });
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  }, 'image/png'),
);

document.querySelectorAll('.js-again').forEach((b) => b.addEventListener('click', () => { $('file-input').value = ''; show('dropzone'); }));
$('file-input').addEventListener('change', (e) => handle(e.target.files[0]));

const dz = $('dropzone');
dz.addEventListener('dragover', (e) => { e.preventDefault(); dz.classList.add('drag-over'); });
dz.addEventListener('dragleave', () => dz.classList.remove('drag-over'));
dz.addEventListener('drop', (e) => { e.preventDefault(); dz.classList.remove('drag-over'); handle(e.dataTransfer.files[0]); });
document.addEventListener('paste', (e) => {
  const f = [...e.clipboardData.files].find((f) => f.type.startsWith('image/'));
  if (f && !$('dropzone').classList.contains('hidden')) handle(f);
});

// tabs
function tab(name) {
  for (const t of ['image', 'video']) {
    $(`tab-${t}`).classList.toggle('active', t === name);
    $(`tab-${t}`).setAttribute('aria-selected', t === name);
    $(`panel-${t}`).classList.toggle('hidden', t !== name);
  }
}
document.querySelectorAll('[data-tab]').forEach((el) => el.addEventListener('click', (e) => {
  if (el.tagName === 'A') e.preventDefault();
  tab(el.dataset.tab);
  if (el.tagName === 'A') $('tab-image').scrollIntoView({ behavior: 'smooth', block: 'start' });
}));

// ── video: one worker per job (fresh wasm memory each time) ──
const vViews = ['v-dropzone', 'v-busy', 'v-refused', 'v-result'];
let vJob = null; // { worker, t0, timer, urls }
function vShow(v) {
  for (const id of vViews) $(id).classList.toggle('hidden', id !== v);
  $('v-error').classList.add('hidden');
}
function vReset() {
  if (vJob) {
    vJob.worker.terminate();
    clearInterval(vJob.timer);
    vJob.urls.forEach((u) => URL.revokeObjectURL(u));
  }
  vJob = null;
}
function vFail(msg) {
  vReset();
  vShow('v-dropzone');
  $('v-error').textContent = msg;
  $('v-error').classList.remove('hidden');
}

async function vHandle(file) {
  if (!file) return;
  if (!('VideoDecoder' in window) || !('VideoEncoder' in window)) return vFail('This browser has no WebCodecs; use a recent Chrome or Edge, or the CLI.');
  if (!/\.(mp4|mov|m4v)$/i.test(file.name) && !/^video\/(mp4|quicktime)$/.test(file.type)) return vFail('Use an MP4 or MOV file (WebM/MKV: use the CLI).');
  vReset();
  const worker = new Worker('video-worker.js');
  const src = URL.createObjectURL(file);
  vJob = { worker, t0: performance.now(), urls: [src], name: file.name.replace(/\.[^.]+$/, '') };
  vJob.timer = setInterval(() => ($('v-time').textContent = Math.round((performance.now() - vJob.t0) / 1000)), 500);
  $('v-stage').textContent = 'Reading the file…';
  $('v-bar').style.width = '0%';
  vShow('v-busy');
  worker.onmessage = ({ data: d }) => {
    if (d.progress) {
      $('v-stage').textContent = d.progress;
      $('v-bar').style.width = `${(100 * d.frac).toFixed(1)}%`;
    } else if (d.error) {
      vFail(`Could not process this video: ${d.error}`);
    } else if (d.refused) {
      $('v-refused-log').textContent = d.log;
      clearInterval(vJob.timer);
      vShow('v-refused');
    } else if (d.done) {
      clearInterval(vJob.timer);
      const secs = Math.round((performance.now() - vJob.t0) / 1000);
      const out = URL.createObjectURL(new Blob([d.mp4], { type: 'video/mp4' }));
      vJob.urls.push(out);
      $('v-before').src = src;
      $('v-after').src = out;
      $('v-download').href = out;
      $('v-download').download = `${vJob.name}_clean.mp4`;
      const m = d.log.match(/detected (\S+) x([\d.]+) at \((\d+),(\d+)\)/);
      if (m) vZoomAt(+m[3], +m[4], +m[2]);
      $('v-badge').textContent = `✓ Removed ${m ? m[1].replace(/\.png$/, '') : 'mark'} · ${secs} s · ${(d.mp4.byteLength / 1e6).toFixed(1)} MB`;
      $('v-log').textContent = d.log;
      vShow('v-result');
    }
  };
  worker.onerror = (e) => vFail(`Worker failed: ${e.message}`);
  worker.postMessage({ buf: await file.arrayBuffer() });
}

$('v-input').addEventListener('change', (e) => vHandle(e.target.files[0]));
document.querySelectorAll('.js-v-again').forEach((b) => b.addEventListener('click', () => { vReset(); $('v-input').value = ''; vShow('v-dropzone'); }));
const vdz = $('v-dropzone');
vdz.addEventListener('dragover', (e) => { e.preventDefault(); vdz.classList.add('drag-over'); });
vdz.addEventListener('dragleave', () => vdz.classList.remove('drag-over'));
vdz.addEventListener('drop', (e) => { e.preventDefault(); vdz.classList.remove('drag-over'); vHandle(e.dataTransfer.files[0]); });
$('v-play').addEventListener('click', () => {
  const [a, b] = [$('v-before'), $('v-after')];
  a.currentTime = b.currentTime = 0;
  a.play();
  b.play();
});
// keep the two players in step when either is seeked
for (const [a, b] of [['v-before', 'v-after'], ['v-after', 'v-before']]) {
  $(a).addEventListener('seeked', () => { if (Math.abs($(a).currentTime - $(b).currentTime) > 0.05) $(b).currentTime = $(a).currentTime; });
}

// zoomed mark area of both players, redrawn while playing / after seeking
let vZ = null;
function vZoomAt(x, y, scale) {
  vZ = { x: x + 10 * scale, y: y + 5 * scale };
}
function vZoomDraw() {
  const a = $('v-before');
  if (!vZ || !a.videoWidth) return;
  const [W, H] = [a.videoWidth, a.videoHeight];
  const cw = Math.round(W * 0.12), ch = Math.round((cw * 9) / 16);
  const sx = Math.max(0, Math.min(W - cw, vZ.x - cw / 2)), sy = Math.max(0, Math.min(H - ch, vZ.y - ch / 2));
  for (const [v, c] of [['v-before', 'vz-before'], ['v-after', 'vz-after']]) {
    const g = $(c).getContext('2d');
    g.imageSmoothingEnabled = false;
    g.drawImage($(v), sx, sy, cw, ch, 0, 0, $(c).width, $(c).height);
  }
}
(function loop() { if (!$('v-result').classList.contains('hidden')) vZoomDraw(); requestAnimationFrame(loop); })();
