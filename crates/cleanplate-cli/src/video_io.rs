//! Video path: ffmpeg decode -> core::session (pass 1, detect, rebuild, paste) -> ffmpeg encode.
use crate::ffmpeg::{encoder_args, encoders, pick_codec, probe, Decoder};
use crate::{finish, partial_path, Outcome};
use anyhow::{Context, Result};
use cleanplate_core::geom::{remap, Border, Interp};
use cleanplate_core::session::{Rect, Session};
use cleanplate_core::video::{Guide, Params};
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

/// Clean one video into `out`. `log` receives the progress lines (time-stamped).
pub fn clean(
    inp: &Path,
    out: &Path,
    o: &VideoOpts,
    log: &mut dyn FnMut(String),
) -> Result<Outcome> {
    let t0 = Instant::now();
    let mut log = |m: String| log(format!("[{:6.1}s] {m}", t0.elapsed().as_secs_f32()));
    if out.exists() && std::fs::canonicalize(inp)? == std::fs::canonicalize(out)? {
        anyhow::bail!(
            "the output {} is the input file, choose another name",
            out.display()
        );
    }
    let p = probe(inp)?;
    let (w, h) = (p.w, p.h);
    // resolve the encoder before the expensive work so a missing one fails fast
    let avail = encoders();
    let codec = pick_codec(&o.codec, out, |e| avail.iter().any(|a| a == e))?;
    log(format!(
        "{w}x{h} @ {}, audio={}, colour={:?}",
        p.fps, p.has_audio, p.color
    ));
    let mut s = Session::new(w, h).map_err(anyhow::Error::msg)?;
    // pass 1
    for f in Decoder::open(inp, w, h)? {
        s.push(&f);
    }
    let n = s.frames();
    anyhow::ensure!(
        n > 0,
        "no video frames could be decoded from {}",
        inp.display()
    );
    let (cw, ch) = s.corner();
    log(format!("pass 1: {n} frames, corner crop {cw}x{ch}"));
    let found = match s.detect(o.min_ncc, o.opaque, o.guide.is_some(), &mut log) {
        Ok(f) => f,
        Err(r) => {
            return Ok(Outcome::Refused(format!(
                "{r} - leaving the video untouched"
            )))
        }
    };
    let guide = match (&o.guide, found.opaque) {
        (Some(g), true) => Some(load_guide(g, w, found.rect, n)?),
        _ => None,
    };
    let mut params = o.params.clone();
    params.tv_range_white = match p.color[0].as_deref() {
        None | Some("tv") | Some("unknown") => 235.0,
        _ => 255.0,
    };
    s.rebuild(&params, guide.as_ref(), &mut log);
    // pass 2: paste the rebuilt region (feathered) into the untouched frames and encode
    // encode to a hidden partial file, renamed into place only when complete
    let tmp = partial_path(out);
    let mut enc = Command::new("ffmpeg")
        .args(encoder_args(&codec, inp, &tmp, &p))
        .env("SVT_LOG", "1") // SVT-AV1 prints a banner on stderr by default; errors only
        .stdin(Stdio::piped())
        .spawn()
        .context("running ffmpeg encoder")?;
    let mut sink = enc.stdin.take().context("encoder stdin")?;
    // never leave a partial or short file behind: any failure removes the output
    let written = (|| -> Result<usize> {
        let mut count = 0;
        for (t, mut f) in Decoder::open(inp, w, h)?.enumerate().take(n) {
            s.paste(t, &mut f);
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
    let failed = failed.or_else(|| {
        finish(&tmp, out)
            .err()
            .map(|e| format!("moving the result into place: {e}"))
    });
    if let Some(msg) = failed {
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("{msg}, no output written");
    }
    let mb = std::fs::metadata(out)
        .map(|m| m.len() as f64 / 1e6)
        .unwrap_or(0.0);
    log(format!("done -> {} ({codec}), {mb:.1} MB", out.display()));
    Ok(Outcome::Cleaned)
}

/// Guide clip (same clip, lower resolution, already cleaned) resampled onto the region grid, frame for frame.
fn load_guide(path: &Path, w: usize, r: Rect, n: usize) -> Result<Guide> {
    let (rx, ry, rw, rh) = (r.x, r.y, r.w, r.h);
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
