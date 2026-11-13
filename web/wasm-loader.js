// Loads cleanplate into a worker: the multi-threaded build (wasm32-wasip1-threads + rayon) when the page is
// cross-origin isolated, else the single-threaded build. Usage (in a worker): importScripts('wasm-loader.js');
// const { exports, threads } = await loadCleanplate();
// The multi-threaded module imports a shared memory, a few WASI preview1 calls and wasi `thread-spawn`; each spawned
// thread is a Web Worker (wasi-thread.js) instantiating the same module on the same memory.

/** WASI preview1 calls the Rust std uses here. Shared memory: copy before decoding, never hand it to Web APIs. */
function wasiImports(memory) {
  const dv = () => new DataView(memory.buffer);
  const u8 = () => new Uint8Array(memory.buffer);
  return {
    fd_write(fd, iovs, n, nwritten) {
      let total = 0, text = '';
      for (let i = 0; i < n; i++) {
        const p = dv().getUint32(iovs + 8 * i, true), len = dv().getUint32(iovs + 8 * i + 4, true);
        text += new TextDecoder().decode(u8().slice(p, p + len));
        total += len;
      }
      if (text.trim()) (fd === 2 ? console.warn : console.log)('[cleanplate]', text.trimEnd());
      dv().setUint32(nwritten, total, true);
      return 0;
    },
    random_get(p, len) {
      const tmp = new Uint8Array(len);
      for (let o = 0; o < len; o += 65536) crypto.getRandomValues(tmp.subarray(o, Math.min(len, o + 65536)));
      u8().set(tmp, p);
      return 0;
    },
    clock_time_get(_id, _precision, out) {
      dv().setBigUint64(out, BigInt(Math.round((performance.timeOrigin + performance.now()) * 1e6)), true);
      return 0;
    },
    environ_sizes_get(count, size) {
      dv().setUint32(count, 0, true);
      dv().setUint32(size, 0, true);
      return 0;
    },
    environ_get: () => 0,
    sched_yield: () => 0,
    proc_exit(code) {
      throw new Error(`cleanplate exited (${code})`);
    },
  };
}

/** Minimum pages of the imported memory, read from the module's import section. */
function importedMemoryMin(bytes) {
  const b = new Uint8Array(bytes);
  let i = 8;
  const leb = () => {
    let r = 0, s = 0, x;
    do { x = b[i++]; r |= (x & 0x7f) << s; s += 7; } while (x & 0x80);
    return r >>> 0;
  };
  const str = () => { const n = leb(); i += n; };
  while (i < b.length) {
    const id = b[i++], size = leb(), end = i + size;
    if (id === 2) {
      for (let n = leb(); n > 0; n--) {
        str(); str();
        const kind = b[i++];
        if (kind === 0) leb();
        else if (kind === 1) { i++; const f = b[i++]; leb(); if (f & 1) leb(); }
        else if (kind === 2) { const f = b[i++]; const min = leb(); if (f & 1) leb(); return min; }
        else if (kind === 3) i += 2;
      }
    }
    i = end;
  }
  return 17;
}

async function loadMultiThreaded() {
  const bytes = await (await fetch('cleanplate-mt.wasm')).arrayBuffer();
  const module = await WebAssembly.compile(bytes);
  const memory = new WebAssembly.Memory({ initial: importedMemoryMin(bytes), maximum: 65536, shared: true });
  const n = Math.max(2, Math.min(16, navigator.hardwareConcurrency || 4));
  // Thread workers are started, given the module and memory, and instantiated *before* any wasm runs: rayon blocks
  // this thread inside cp_threads until its threads are up, and while it is blocked Chrome neither starts a new
  // worker nor completes the first transfer of a module, so thread-spawn may only send two numbers.
  const pool = await Promise.all(
    Array.from({ length: n }, () => new Promise((resolve, reject) => {
      const w = new Worker('wasi-thread.js');
      w.onmessage = (m) => m.data === 'ready' && resolve(w);
      w.onerror = (ev) => reject(new Error(`thread worker: ${ev.message}`));
    })),
  );
  let nextTid = 1;
  const instance = await WebAssembly.instantiate(module, {
    env: { memory },
    wasi_snapshot_preview1: wasiImports(memory),
    wasi: {
      'thread-spawn'(arg) {
        const w = pool.pop();
        if (!w) return -6; // EAGAIN: the pool is sized for rayon's threads
        const tid = nextTid++;
        w.postMessage({ tid, arg });
        return tid;
      },
    },
  });
  await Promise.all(
    pool.map((w) => new Promise((resolve) => {
      w.onmessage = (m) => m.data === 'instantiated' && resolve();
      w.postMessage({ module, memory });
    })),
  );
  const e = instance.exports;
  // what wasi-libc's _initialize does for a reactor: the main thread's pointer, then the constructors (a cdylib
  // exports neither by default; build with --export=__wasi_init_tp --export=__wasm_call_ctors). Without the first,
  // the first pthread_create deadlocks.
  e.__wasi_init_tp();
  e.__wasm_call_ctors();
  return { exports: e, threads: e.cp_threads(n) };
}

async function loadSingleThreaded() {
  const bytes = await (await fetch('cleanplate.wasm')).arrayBuffer();
  const { instance } = await WebAssembly.instantiate(bytes, {});
  return { exports: instance.exports, threads: 1 };
}

/** `single`: skip the multi-threaded build (the page's watchdog asks for it when that build did not come up). */
async function loadCleanplate(single = false) {
  if (!single && self.crossOriginIsolated && typeof SharedArrayBuffer !== 'undefined') {
    try {
      return await loadMultiThreaded();
    } catch (err) {
      console.warn('[cleanplate] multi-threaded build unavailable, using one thread:', err);
    }
  }
  return loadSingleThreaded();
}
