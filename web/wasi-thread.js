// One wasi-threads thread of cleanplate-mt.wasm, started ahead by wasm-loader.js: first {module, memory} (instantiate
// and wait), then {tid, arg} from thread-spawn (run the thread).
importScripts('wasm-loader.js');
let instance = null;
onmessage = ({ data }) => {
  if (data.module) {
    instance = new WebAssembly.Instance(data.module, {
      env: { memory: data.memory },
      wasi_snapshot_preview1: wasiImports(data.memory),
      wasi: { 'thread-spawn': () => -6 }, // only the main instance spawns (rayon's pool)
    });
    postMessage('instantiated');
    return;
  }
  instance.exports.wasi_thread_start(data.tid, data.arg);
  close();
};
postMessage('ready');
