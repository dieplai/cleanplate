// Runs cleanplate.wasm off the main thread. Message in: {rgb: Uint8Array, w, h, lossy}; out: {status, log, rgb} | {error}.
// Result block layout (see crates/cleanplate-wasm): u32 LE [status, log_len, rgb_len] ++ log ++ rgb.
const load = () =>
  fetch('cleanplate.wasm')
    .then((r) => r.arrayBuffer())
    .then((b) => WebAssembly.instantiate(b, {}))
    .then(({ instance }) => instance.exports);
let ready = load();

onmessage = async ({ data: { rgb, w, h, lossy } }) => {
  try {
    const e = await ready;
    const inp = e.cp_alloc(rgb.length);
    new Uint8Array(e.memory.buffer, inp, rgb.length).set(rgb);
    const r = e.cp_remove(inp, w, h, lossy ? 1 : 0);
    e.cp_free(inp, rgb.length);
    // memory may have grown during the call: make views only now
    const dv = new DataView(e.memory.buffer, r, 12);
    const [status, ll, rl] = [0, 4, 8].map((o) => dv.getUint32(o, true));
    const log = new TextDecoder().decode(new Uint8Array(e.memory.buffer, r + 12, ll));
    const out = new Uint8Array(e.memory.buffer, r + 12 + ll, rl).slice();
    e.cp_free(r, 12 + ll + rl);
    postMessage({ status, log, rgb: out }, [out.buffer]);
  } catch (err) {
    ready = load(); // a trap can leave the instance in a bad state
    postMessage({ error: String(err) });
  }
};
