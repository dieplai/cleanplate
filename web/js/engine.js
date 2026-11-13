// Cleaning one file. Every job gets its own worker (terminated afterwards, so wasm memory is returned at once), a
// watchdog that falls back to the one-thread build, and the optional server path (deploy/server.py).

export const CORES = navigator.hardwareConcurrency || 4;
export const RAM_GB = navigator.deviceMemory || 4;
/** Threads the in-browser engine can use here (0 when memory cannot be shared between threads). */
export const LOCAL_THREADS =
  self.crossOriginIsolated && typeof SharedArrayBuffer !== 'undefined' ? Math.max(2, Math.min(16, CORES, (navigator.deviceMemory || 8) * 2)) : 1;
/** Memory the in-browser engine may use (MB): 40% of the reported RAM, 1-3 GB. Phones close tabs far below 4 GB. */
export const BUDGET_MB = Math.round(Math.min(3000, Math.max(1000, RAM_GB * 1024 * 0.4)));
/** Measured with 12 threads: 4.3 MP -> 273 MB, 17 MP -> 1067 MB. */
export const imageNeedMB = (w, h) => Math.round(150 + 62e-6 * w * h);
/** Before demuxing only the file size is known: ~1 MB of memory per 20 KB of an 8 s Veo clip, plus input and output. */
export const videoGuessMB = (bytes) => Math.round(350 + (6 * bytes) / 2 ** 20);
export const SERVER_MAX_MB = 95;
const WATCHDOG_MS = 10000;

// ── server (deploy/server.py) ──
export let SERVER = null;
export const serverReady = fetch('api/health')
  .then((r) => (r.ok ? r.json() : null))
  .then((j) => (SERVER = j?.ok ? j : null))
  .catch(() => null);
/** A few cores in the browser lose to the server once upload time counts, many cores win. */
export const preferServer = () => SERVER && LOCAL_THREADS < 6;

/** POST a file to the server: {status: 'cleaned', blob, log, secs} | {status: 'refused', log}. */
export function serverClean(file, onUpload) {
  return new Promise((resolve, reject) => {
    const x = new XMLHttpRequest();
    x.open('POST', `api/clean?name=${encodeURIComponent(file.name)}`);
    x.responseType = 'blob';
    x.upload.onprogress = (e) => e.lengthComputable && onUpload?.(e.loaded / e.total);
    x.upload.onload = () => onUpload?.(1);
    x.onload = async () => {
      const log = decodeURIComponent(x.getResponseHeader('X-Cleanplate-Log') || '');
      if (x.status === 200) return resolve({ status: 'cleaned', blob: x.response, log, secs: +x.getResponseHeader('X-Cleanplate-Seconds') });
      let j = {};
      try {
        j = JSON.parse(await x.response.text());
      } catch {
        /* not JSON (a proxy error page) */
      }
      if (x.status === 422) resolve({ status: 'refused', log: j.log || '' });
      else if (x.status === 429) reject(new Error('too many files from your connection, wait a minute'));
      else reject(new Error(j.error || `server answered ${x.status}`));
    };
    x.onerror = () => reject(new Error('network error'));
    x.send(file);
  });
}

/** Start `script` in a worker and resolve once its engine reports {loaded}. Falls back to one thread after the
 * watchdog. `onMessage` receives every later message. */
function startWorker(script, threads, onMessage) {
  return new Promise((resolve, reject) => {
    const launch = (single) => {
      const w = new Worker(`${script}?${single ? 'single' : `threads=${threads}`}`);
      let loaded = false;
      const dog = single ? 0 : setTimeout(() => !loaded && (w.terminate(), launch(true)), WATCHDOG_MS);
      w.onmessage = ({ data }) => {
        if (data.loaded) {
          loaded = true;
          clearTimeout(dog);
          resolve({ worker: w, threads: data.loaded });
        } else onMessage(data);
      };
      w.onerror = (e) => (clearTimeout(dog), reject(new Error(`worker failed: ${e.message}`)));
    };
    launch(false);
  });
}

/** Decode an image file to RGBA pixels (colour values as stored, no colour management). */
export async function decodeImage(file) {
  const bmp = await createImageBitmap(file, { colorSpaceConversion: 'none', premultiplyAlpha: 'none' });
  const c = new OffscreenCanvas(bmp.width, bmp.height);
  const g = c.getContext('2d', { willReadFrequently: true });
  g.drawImage(bmp, 0, 0);
  bmp.close();
  return g.getImageData(0, 0, c.width, c.height);
}

const toRgb = (img) => {
  const rgb = new Uint8Array(img.width * img.height * 3);
  for (let i = 0, j = 0; i < img.data.length; i += 4, j += 3) [rgb[j], rgb[j + 1], rgb[j + 2]] = [img.data[i], img.data[i + 1], img.data[i + 2]];
  return rgb;
};

/** Clean a still in this tab. Resolves {status, log, src: ImageData, out?: ImageData, threads}. */
export async function cleanImageLocal(file, src, threads) {
  let job;
  const result = new Promise((resolve, reject) => {
    job = startWorker('worker.js', threads, (d) => (d.error ? reject(new Error(d.error)) : resolve(d)));
    job.catch(reject);
  });
  const { worker, threads: used } = await job;
  try {
    worker.postMessage({ rgb: toRgb(src), w: src.width, h: src.height, lossy: file.type !== 'image/png' });
    const d = await result;
    if (d.status !== 0) return { status: 'refused', log: d.log, src, threads: used };
    const out = new ImageData(new Uint8ClampedArray(src.data), src.width, src.height);
    for (let i = 0, j = 0; i < out.data.length; i += 4, j += 3) [out.data[i], out.data[i + 1], out.data[i + 2]] = [d.rgb[j], d.rgb[j + 1], d.rgb[j + 2]];
    return { status: 'cleaned', log: d.log, src, out, threads: used };
  } finally {
    worker.terminate();
  }
}

/** Encode cleaned pixels: png (lossless, default), jpg (q 0.95) or webp. */
export function encodeImage(img, format) {
  const c = new OffscreenCanvas(img.width, img.height);
  c.getContext('2d').putImageData(img, 0, 0);
  const type = { png: 'image/png', jpg: 'image/jpeg', webp: 'image/webp' }[format];
  return c.convertToBlob({ type, quality: format === 'png' ? undefined : 0.95 });
}

/** Clean a video in this tab. onProgress(stage, fraction). Resolves {status, log, blob?, threads} or rejects; a job
 * larger than the memory budget rejects with err.tooBig = MB. */
export async function cleanVideoLocal(file, threads, onProgress) {
  let job;
  const result = new Promise((resolve, reject) => {
    job = startWorker('video-worker.js', threads, (d) => {
      if (d.progress) onProgress(d.progress, d.frac);
      else if (d.error) reject(new Error(d.error));
      else if (d.tooBig) reject(Object.assign(new Error(`needs about ${d.tooBig} MB of memory, more than ${BUDGET_MB} MB here`), { tooBig: d.tooBig }));
      else if (d.refused) resolve({ status: 'refused', log: d.log });
      else if (d.done) resolve({ status: 'cleaned', log: d.log, blob: new Blob([d.mp4], { type: 'video/mp4' }) });
    });
    job.catch(reject);
  });
  const { worker, threads: used } = await job;
  try {
    const buf = await file.arrayBuffer();
    worker.postMessage({ buf, budgetMB: BUDGET_MB }, [buf]);
    return { ...(await result), threads: used };
  } finally {
    worker.terminate();
  }
}
