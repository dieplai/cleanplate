//! Video path: pass 1 (corner crops + thumbnails), detection, rebuild, pass 2 (paste + encode).
use crate::ffmpeg::{encoder_args, has_encoder, probe, Decoder};
use anyhow::{Context, Result};
use cleanplate_core::detect::{detect_video, sample_step, shot_cuts};
use cleanplate_core::geom::{remap, resize_area, Border, Interp};
use cleanplate_core::video::{place_alpha, prepare_alpha, rebuild, region_bounds, Guide, Params};
use cleanplate_core::Plane;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

pub struct VideoOpts {
    pub codec: String,
    pub params: Params,
    pub min_ncc: f32,
    pub opaque: i32,
    pub guide: Option<std::path::PathBuf>,
}

/// Exit code: 0 done, 2 refused (no mark found with confidence; nothing written).
pub fn run(inp: &Path, out: &Path, o: &VideoOpts) -> Result<i32> {
    let t0 = Instant::now();
    let log = |m: String| println!("[{:6.1}s] {m}", t0.elapsed().as_secs_f32());
    if out.exists() && std::fs::canonicalize(inp)? == std::fs::canonicalize(out)? {
        anyhow::bail!(
            "the output {} is the input file; choose another name",
            out.display()
        );
    }
    let p = probe(inp)?;
    let (w, h) = (p.w, p.h);
    anyhow::ensure!(
        w % 2 == 0 && h % 2 == 0,
        "odd frame size {w}x{h} is not supported (4:2:0 needs even dimensions)"
    );
    log(format!(
        "{w}x{h} @ {}, audio={}, colour={:?}",
        p.fps, p.has_audio, p.color
    ));
    let (cw, ch) = ((w / 2).min(480) & !1, (h / 2).min(320) & !1);
    let (cx0, cy0) = (w - cw, h - ch);
    // pass 1: keep only the bottom-right corner (Y, U, V) and a 64×36 luma thumbnail per frame
    let (mut cy, mut cu, mut cv, mut thumbs) = (vec![], vec![], vec![], vec![]);
    for f in Decoder::open(inp, w, h)? {
        let (y, rest) = f.split_at(w * h);
        let (u, v) = rest.split_at(w * h / 4);
        let crop = |pl: &[u8], pw: usize, x0: usize, y0: usize, cw: usize, ch: usize| -> Vec<u8> {
            (0..ch)
                .flat_map(|r| {
                    pl[(y0 + r) * pw + x0..(y0 + r) * pw + x0 + cw]
                        .iter()
                        .copied()
                })
                .collect()
        };
        cy.push(crop(y, w, cx0, cy0, cw, ch));
        cu.push(crop(u, w / 2, cx0 / 2, cy0 / 2, cw / 2, ch / 2));
        cv.push(crop(v, w / 2, cx0 / 2, cy0 / 2, cw / 2, ch / 2));
        thumbs.push(resize_area(&Plane::from_u8(w, h, y), 64, 36));
    }
    let n = cy.len();
    log(format!("pass 1: {n} frames, corner crop {cw}x{ch}"));
    let shots = shot_cuts(&thumbs);
    log(format!("shots: {} -> {:?}", shots.len(), shots));
    // only the frames detection samples become f32 (≈120), not the whole clip
    let corners: Vec<Plane> = cy
        .iter()
        .step_by(sample_step(n))
        .map(|c| Plane::from_u8(cw, ch, c))
        .collect();
    let refs: Vec<&Plane> = corners.iter().collect();
    let det = match detect_video(&refs, w, h, o.min_ncc) {
        Ok(d) => d,
        Err(r) => {
            println!("no mark found with confidence (best ncc {:.2} < {:.2}) — leaving the video untouched", r.best_ncc, r.floor);
            return Ok(2);
        }
    };
    log(format!(
        "detected {} x{:.2} at ({},{}) ncc={:.3}{}",
        det.mark.name,
        det.scale,
        det.x + cx0,
        det.y + cy0,
        det.ncc,
        if det.at_known {
            " (known placement)"
        } else {
            ""
        }
    ));
    drop(corners);
    let prep = prepare_alpha(&det, o.opaque, o.guide.is_some());
    if prep.op >= 0 {
        log(format!("opaque mode: support dilated {}px", prep.op));
    }
    let r = region_bounds(&prep, cw, ch);
    let ay = place_alpha(&prep, &r);
    let region = |c: &[u8], pw: usize, x0: usize, y0: usize, rw: usize, rh: usize| -> Plane {
        Plane {
            w: rw,
            h: rh,
            data: (0..rh)
                .flat_map(|yy| {
                    c[(y0 + yy) * pw + x0..(y0 + yy) * pw + x0 + rw]
                        .iter()
                        .map(|&v| v as f32)
                })
                .collect(),
        }
    };
    let jy: Vec<Plane> = cy
        .iter()
        .map(|c| region(c, cw, r.x0, r.y0, r.w, r.h))
        .collect();
    let ju: Vec<Plane> = cu
        .iter()
        .map(|c| region(c, cw / 2, r.x0 / 2, r.y0 / 2, r.w / 2, r.h / 2))
        .collect();
    let jv: Vec<Plane> = cv
        .iter()
        .map(|c| region(c, cw / 2, r.x0 / 2, r.y0 / 2, r.w / 2, r.h / 2))
        .collect();
    drop((cy, cu, cv));
    let guide = match (&o.guide, prep.op >= 0) {
        (Some(g), true) => Some(load_guide(g, w, cx0 + r.x0, cy0 + r.y0, r.w, r.h, n)?),
        _ => None,
    };
    let mut params = o.params.clone();
    params.tv_range_white = match p.color[0].as_deref() {
        None | Some("tv") | Some("unknown") => 235.0,
        _ => 255.0,
    };
    let rb = rebuild(&jy, &ju, &jv, &ay, prep.op, &shots, &params, guide.as_ref());
    for s in &rb.shot_logs {
        log(format!(
            "shot {}-{}: motion={:.2}px K={} lambda={:.1} clean-share={:.2}",
            s.s0, s.s1, s.motion, s.k, s.lam, s.clean_share
        ));
    }
    // pass 2: paste the rebuilt region (feathered) into the untouched frames and encode
    let ext = out
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let codec = if o.codec == "auto" {
        if ext == "mov" {
            "prores".to_string()
        } else if has_encoder("libx264") {
            "h264x".to_string()
        } else {
            "av1".to_string()
        }
    } else {
        o.codec.clone()
    };
    let mut enc = Command::new("ffmpeg")
        .args(encoder_args(&codec, inp, out, &p))
        .stdin(Stdio::piped())
        .spawn()
        .context("running ffmpeg encoder")?;
    let mut sink = enc.stdin.take().context("encoder stdin")?;
    let (x0, y0) = (cx0 + r.x0, cy0 + r.y0);
    // never leave a partial or short file behind: any failure removes the output
    let written = (|| -> Result<usize> {
        let mut count = 0;
        for (t, mut f) in Decoder::open(inp, w, h)?.enumerate().take(n) {
            let (yp, rest) = f.split_at_mut(w * h);
            let (up, vp) = rest.split_at_mut(w * h / 4);
            paste(yp, w, x0, y0, &rb.y[t], &rb.feather_y);
            paste(up, w / 2, x0 / 2, y0 / 2, &rb.u[t], &rb.feather_c);
            paste(vp, w / 2, x0 / 2, y0 / 2, &rb.v[t], &rb.feather_c);
            sink.write_all(&f)?;
            count += 1;
        }
        Ok(count)
    })();
    drop(sink);
    let st = enc.wait();
    let failed = match (&written, &st) {
        (Ok(c), Ok(s)) if *c == n && s.success() => None,
        (Ok(c), Ok(s)) if s.success() => Some(format!("pass 2 decoded {c} of {n} frames")),
        (_, Ok(s)) if !s.success() => Some(format!("ffmpeg encoder failed ({s})")),
        (Err(e), _) => Some(format!("writing frames: {e}")),
        (_, Err(e)) => Some(format!("waiting for ffmpeg: {e}")),
        _ => Some("encoding failed".into()),
    };
    if let Some(msg) = failed {
        let _ = std::fs::remove_file(out);
        anyhow::bail!("{msg}; no output written");
    }
    let mb = std::fs::metadata(out)
        .map(|m| m.len() as f64 / 1e6)
        .unwrap_or(0.0);
    log(format!("done -> {} ({codec}), {mb:.1} MB", out.display()));
    Ok(0)
}

/// feather·O + (1 − feather)·source, rounded like the reference (+0.5 then truncation).
fn paste(plane: &mut [u8], pw: usize, x0: usize, y0: usize, o: &Plane, feather: &Plane) {
    for yy in 0..o.h {
        for xx in 0..o.w {
            let i = (y0 + yy) * pw + x0 + xx;
            let f = feather.at(xx, yy);
            let v = f * o.at(xx, yy) + (1.0 - f) * plane[i] as f32 + 0.5;
            plane[i] = v.clamp(0.0, 255.0) as u8;
        }
    }
}

/// Guide clip (same clip, lower resolution, already cleaned) resampled onto the region grid, frame for frame.
fn load_guide(
    path: &Path,
    w: usize,
    rx: usize,
    ry: usize,
    rw: usize,
    rh: usize,
    n: usize,
) -> Result<Guide> {
    let gp = probe(path)?;
    let (gw, gh) = (gp.w, gp.h);
    anyhow::ensure!(
        gw % 2 == 0 && gh % 2 == 0,
        "guide: odd frame size {gw}x{gh} is not supported"
    );
    let sg = w as f32 / gw as f32;
    let maps = |pw: usize, ph: usize, ox: usize, oy: usize| -> (Plane, Plane) {
        let mx = Plane {
            w: pw,
            h: ph,
            data: (0..pw * ph)
                .map(|k| (ox + k % pw) as f32 + 0.5)
                .map(|v| v / sg - 0.5)
                .collect(),
        };
        let my = Plane {
            w: pw,
            h: ph,
            data: (0..pw * ph)
                .map(|k| (oy + k / pw) as f32 + 0.5)
                .map(|v| v / sg - 0.5)
                .collect(),
        };
        (mx, my)
    };
    let (ymx, ymy) = maps(rw, rh, rx, ry);
    let (cmx, cmy) = maps(rw / 2, rh / 2, rx / 2, ry / 2);
    let mut g = Guide {
        y: vec![],
        u: vec![],
        v: vec![],
    };
    for f in Decoder::open(path, gw, gh)?.take(n) {
        let (y, rest) = f.split_at(gw * gh);
        let (u, v) = rest.split_at(gw * gh / 4);
        g.y.push(remap(
            &Plane::from_u8(gw, gh, y),
            &ymx,
            &ymy,
            Interp::Cubic,
            Border::Replicate,
        ));
        g.u.push(remap(
            &Plane::from_u8(gw / 2, gh / 2, u),
            &cmx,
            &cmy,
            Interp::Cubic,
            Border::Replicate,
        ));
        g.v.push(remap(
            &Plane::from_u8(gw / 2, gh / 2, v),
            &cmx,
            &cmy,
            Interp::Cubic,
            Border::Replicate,
        ));
    }
    anyhow::ensure!(!g.y.is_empty(), "guide has no frames");
    Ok(g)
}
