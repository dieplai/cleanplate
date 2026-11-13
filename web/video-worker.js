// Video in the browser: mp4box demux → WebCodecs decode (twice) → cleanplate.wasm → WebCodecs H.264 encode → mp4-muxer.
// The audio track is copied without re-encoding. Messages out: {progress} | {log} | {done, mp4, log} | {refused, log} | {error}.
importScripts('vendor/mp4box.all.min.js', 'vendor/mp4-muxer.min.js', 'wasm-loader.js');

const wasm = loadCleanplate(self.location.search.includes('single'));
wasm.then(({ threads }) => postMessage({ loaded: threads }));

function readBlock(e, p) {
  const dv = new DataView(e.memory.buffer, p, 12);
  const [status, ll, rl] = [0, 4, 8].map((o) => dv.getUint32(o, true));
  const log = new TextDecoder().decode(new Uint8Array(e.memory.buffer, p + 12, ll).slice()); // memory may be shared
  e.cp_free(p, 12 + ll + rl);
  return { status, log };
}

function demux(buf) {
  const f = MP4Box.createFile();
  let info = null;
  const samples = {};
  f.onError = (e) => { throw new Error(`MP4 parse error: ${e}`); };
  f.onReady = (i) => {
    info = i;
    for (const t of i.tracks) {
      samples[t.id] = [];
      f.setExtractionOptions(t.id, null, { nbSamples: 1e9 });
    }
    f.start();
  };
  f.onSamples = (id, _u, s) => samples[id].push(...s);
  buf.fileStart = 0;
  f.appendBuffer(buf);
  f.flush();
  if (!info) throw new Error('Not an MP4/MOV file this demo can read (WebM/MKV: use the CLI).');
  return { f, info, samples };
}

function boxBytes(box) {
  const DS = self.DataStream || MP4Box.DataStream;
  const s = new DS(undefined, 0, DS.BIG_ENDIAN);
  box.write(s);
  return new Uint8Array(s.buffer, 8); // drop the box header
}

function videoDescription(f, track) {
  for (const en of f.getTrackById(track.id).mdia.minf.stbl.stsd.entries) {
    const box = en.avcC || en.hvcC || en.vpcC || en.av1C;
    if (box) return boxBytes(box);
  }
}

function aacDescription(f, track) {
  const en = f.getTrackById(track.id).mdia.minf.stbl.stsd.entries[0];
  return en?.esds?.esd?.descs?.[0]?.descs?.[0]?.data;
}

const QP = 14; // H.264 QP for the quantizer mode: visually lossless
/** Peak memory of a job in MB: measured 311 MB for 190 frames, 440 MB for 765 (1080p), 725 MB for 1530 (720p),
 * plus the input file and the encoded output (~3x the input) held in memory. */
const videoNeedMB = (frames, inputBytes) => Math.round(350 + 0.35 * frames + (4 * inputBytes) / 2 ** 20);
const us = (v, ts) => Math.round((1e6 * v) / ts);

/** Decode every sample in presentation order, awaiting `onFrame(frame)` for each (the callee closes it). */
async function decodeAll(cfg, samples, onFrame) {
  const out = [];
  let err = null;
  const dec = new VideoDecoder({ output: (fr) => out.push(fr), error: (e) => (err = e) });
  dec.configure(cfg);
  const drain = async () => { while (out.length) await onFrame(out.shift()); };
  for (const s of samples) {
    if (err) throw err;
    dec.decode(new EncodedVideoChunk({
      type: s.is_sync ? 'key' : 'delta',
      timestamp: us(s.cts, s.timescale),
      duration: us(s.duration, s.timescale),
      data: s.data,
    }));
    await drain();
    while (dec.decodeQueueSize > 6) await new Promise((r) => dec.addEventListener('dequeue', r, { once: true }));
  }
  await dec.flush();
  await drain();
  dec.close();
  if (err) throw err;
}

/** Copy a decoded frame into `dst` as I420 (Y, U, V contiguous). Handles I420 and NV12 decoder output. */
async function toI420(fr, dst, w, h) {
  const rect = fr.visibleRect;
  if (fr.format === 'I420') {
    await fr.copyTo(dst, { rect, layout: [{ offset: 0, stride: w }, { offset: w * h, stride: w / 2 }, { offset: (w * h * 5) / 4, stride: w / 2 }] });
  } else if (fr.format === 'NV12') {
    const tmp = new Uint8Array((w * h * 3) / 2);
    await fr.copyTo(tmp, { rect, layout: [{ offset: 0, stride: w }, { offset: w * h, stride: w }] });
    dst.set(tmp.subarray(0, w * h));
    const q = (w * h) / 4;
    for (let i = 0; i < q; i++) {
      dst[w * h + i] = tmp[w * h + 2 * i];
      dst[w * h + q + i] = tmp[w * h + 2 * i + 1];
    }
  } else {
    throw new Error(`decoder returned ${fr.format} frames, only 8-bit 4:2:0 video is supported here (use the CLI)`);
  }
}

onmessage = async ({ data: { buf, budgetMB } }) => {
  let vid = 0, e = null;
  try {
    e = (await wasm).exports;
    const { f, info, samples } = demux(buf);
    const vt = info.videoTracks[0];
    if (!vt) throw new Error('no video track');
    const at = info.audioTracks[0];
    const vs = samples[vt.id];
    const cfg = {
      codec: vt.codec,
      codedWidth: vt.video.width,
      codedHeight: vt.video.height,
      description: videoDescription(f, vt),
      hardwareAcceleration: 'prefer-software',
    };
    if (!(await VideoDecoder.isConfigSupported(cfg)).supported) throw new Error(`this browser cannot decode ${vt.codec}`);
    const n = vs.length;
    // refuse before the work rather than let the tab run out of memory (see videoNeedMB)
    const needMB = videoNeedMB(n, buf.byteLength);
    if (budgetMB && needMB > budgetMB) return postMessage({ tooBig: needMB });
    const secs = vt.duration / vt.timescale || vs.reduce((a, s) => a + s.duration, 0) / vt.timescale;
    let w = 0, h = 0, frameBuf = null, ptr = 0, fullRange = false;

    // pass 1
    let t = 0;
    await decodeAll(cfg, vs, async (fr) => {
      if (!vid) {
        [w, h] = [fr.visibleRect.width, fr.visibleRect.height];
        fullRange = !!fr.colorSpace?.fullRange;
        vid = e.vid_new(w, h);
        if (!vid) throw new Error(readBlock(e, e.vid_error()).log);
        frameBuf = new Uint8Array((w * h * 3) / 2);
        ptr = e.cp_alloc(frameBuf.length);
      }
      await toI420(fr, frameBuf, w, h);
      fr.close();
      new Uint8Array(e.memory.buffer, ptr, frameBuf.length).set(frameBuf);
      e.vid_push(vid, ptr, frameBuf.length);
      postMessage({ progress: `Pass 1/2 · reading frame ${++t} / ${n}`, frac: (0.25 * t) / n });
    });

    postMessage({ progress: `Rebuilding the hidden pixels from ${t} frames (optical flow)…`, frac: 0.27 });
    const res = readBlock(e, e.vid_solve(vid, fullRange ? 1 : 0));
    if (res.status !== 0) return postMessage({ refused: true, log: res.log });
    postMessage({ log: res.log });

    // pass 2: paste, encode, mux
    const srcBps = (vs.reduce((a, s) => a + s.size, 0) * 8) / Math.max(secs, 0.1);
    const fps = n / Math.max(secs, 0.1);
    const encCfg = {
      codec: 'avc1.640033', // High @ L5.1: up to 4K
      width: w,
      height: h,
      bitrate: Math.round(Math.max(8e6, 3 * srcBps)),
      framerate: fps,
      latencyMode: 'quality',
      avc: { format: 'avc' },
    };
    // constant quality when the encoder offers it (quality first); else a generous bitrate
    const q = { ...encCfg, bitrateMode: 'quantizer' };
    delete q.bitrate;
    const useQ = (await VideoEncoder.isConfigSupported(q).catch(() => ({}))).supported;
    if (useQ) Object.assign(encCfg, q), delete encCfg.bitrate;
    else if (!(await VideoEncoder.isConfigSupported(encCfg)).supported) throw new Error('this browser cannot encode H.264');
    const aacDesc = at && at.codec.startsWith('mp4a') ? aacDescription(f, at) : null;
    const target = new Mp4Muxer.ArrayBufferTarget();
    const muxer = new Mp4Muxer.Muxer({
      target,
      // no frameRate: mp4-muxer only takes an integer there, and 23.976 fps sources are common; timestamps carry it
      video: { codec: 'avc', width: w, height: h },
      audio: aacDesc ? { codec: 'aac', numberOfChannels: at.audio.channel_count, sampleRate: at.audio.sample_rate } : undefined,
      fastStart: 'in-memory',
      firstTimestampBehavior: 'offset',
    });
    let encErr = null;
    const enc = new VideoEncoder({ output: (c, m) => muxer.addVideoChunk(c, m), error: (x) => (encErr = x) });
    enc.configure(encCfg);
    const gop = Math.max(1, Math.round(fps * 2));
    t = 0;
    await decodeAll(cfg, vs, async (fr) => {
      if (encErr) throw encErr;
      await toI420(fr, frameBuf, w, h);
      new Uint8Array(e.memory.buffer, ptr, frameBuf.length).set(frameBuf);
      e.vid_paste(vid, t, ptr, frameBuf.length);
      frameBuf.set(new Uint8Array(e.memory.buffer, ptr, frameBuf.length));
      const out = new VideoFrame(frameBuf, {
        format: 'I420', codedWidth: w, codedHeight: h, timestamp: fr.timestamp, duration: fr.duration ?? undefined,
        colorSpace: fr.colorSpace,
      });
      fr.close();
      enc.encode(out, useQ ? { keyFrame: t % gop === 0, avc: { quantizer: QP } } : { keyFrame: t % gop === 0 });
      out.close();
      while (enc.encodeQueueSize > 4) await new Promise((r) => enc.addEventListener('dequeue', r, { once: true }));
      postMessage({ progress: `Pass 2/2 · writing frame ${++t} / ${n}`, frac: 0.3 + (0.68 * t) / n });
    });
    await enc.flush();
    if (encErr) throw encErr;
    enc.close();
    let note = '';
    if (aacDesc) {
      samples[at.id].forEach((s, i) =>
        muxer.addAudioChunkRaw(s.data, s.is_sync ? 'key' : 'delta', us(s.cts, s.timescale), us(s.duration, s.timescale),
          i === 0 ? { decoderConfig: { codec: at.codec, numberOfChannels: at.audio.channel_count, sampleRate: at.audio.sample_rate, description: aacDesc } } : undefined));
    } else if (at) {
      note = `\naudio track (${at.codec}) not copied: only AAC is supported in the browser`;
    }
    muxer.finalize();
    const mp4 = target.buffer;
    postMessage({ done: true, mp4, log: res.log + `\nencoded H.264 ${useQ ? `QP ${QP}` : `${(encCfg.bitrate / 1e6).toFixed(1)} Mbit/s`}, ${t} frames` + note + `\nwasm memory ${(e.memory.buffer.byteLength / 2 ** 20).toFixed(0)} MB, input ${(buf.byteLength / 2 ** 20).toFixed(1)} MB, output ${(mp4.byteLength / 2 ** 20).toFixed(1)} MB` }, [mp4]);
  } catch (err) {
    postMessage({ error: err?.message || String(err) });
  }
  // ponytail: one worker per job (app.js), so wasm memory is released with the worker instead of freed here

};
