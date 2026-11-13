// Runs cleanplate on stills off the main thread. Message in: {rgb: Uint8Array, w, h, lossy};
// out: {loaded: threads} once, then {status, log, rgb} | {error} per image. `worker.js?single` forces one thread.
// Result block layout (see crates/cleanplate-wasm): u32 LE [status, log_len, rgb_len] ++ log ++ rgb.
importScripts('wasm-loader.js');
const single = self.location.search.includes('single');
let ready = loadCleanplate(single);
ready.then(({ threads }) => postMessage({ loaded: threads }));

onmessage = async ({ data: { rgb, w, h, lossy } }) => {
  try {
    const { exports: e } = await ready;
    const inp = e.cp_alloc(rgb.length);
    new Uint8Array(e.memory.buffer, inp, rgb.length).set(rgb);
    const r = e.cp_remove(inp, w, h, lossy ? 1 : 0);
    e.cp_free(inp, rgb.length);
    // memory may have grown (and may be shared): make views only now, copy before decoding
    const dv = new DataView(e.memory.buffer, r, 12);
    const [status, ll, rl] = [0, 4, 8].map((o) => dv.getUint32(o, true));
    const log = new TextDecoder().decode(new Uint8Array(e.memory.buffer, r + 12, ll).slice());
    const out = new Uint8Array(e.memory.buffer, r + 12 + ll, rl).slice();
    e.cp_free(r, 12 + ll + rl);
    postMessage({ status, log, rgb: out }, [out.buffer]);
  } catch (err) {
    ready = loadCleanplate(single); // a trap can leave the instance in a bad state
    postMessage({ error: String(err) });
  }
};
