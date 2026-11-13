//! Stills (research/image.py): scale-free detection of the Gemini diamond, opacity / offset / fractional size fitted on
//! the image itself, inversion, texture-adaptive TV for lossy inputs.
use crate::detect::Refusal;
use crate::filter::{dilate_rect, gaussian_blur, sobel3};
use crate::geom::{remap, resize_area, resize_cubic, Border, Interp};
use crate::marks::catalog;
use crate::ncc::CorrPlan;
use crate::tv::tv_refine;
use crate::Plane;

pub struct StillResult {
    /// Interleaved RGB, 8-bit, same size as the input.
    pub rgb: Vec<u8>,
    pub log: Vec<String>,
}

/// Margin / mark size at which generators stamp: Gemini stills 2/3, Gemini video 2, Flow (Nano Banana) stills ~1.6.
const PLACE: [f64; 3] = [2.0 / 3.0, 1.6, 2.0];
/// Below this fitted opacity the "mark" is texture: a clean 1024² noise texture matched a 26 px diamond at a known
/// placement (ncc 0.44 > 0.3) but calibrated to 0.078.
pub const MIN_OPACITY: f32 = 0.15;

struct Shape {
    name: &'static str,
    s: Plane, // normalised to plateau 1
    native: f32,
    nsize: usize,
}

fn trim01(a: &Plane) -> Plane {
    let (mut x0, mut y0, mut x1, mut y1) = (a.w, a.h, 0, 0);
    for y in 0..a.h {
        for x in 0..a.w {
            if a.at(x, y) > 0.01 {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
    }
    a.crop(x0, y0, x1 - x0 + 1, y1 - y0 + 1)
}

fn median(v: &mut [f32]) -> f32 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

fn shapes() -> Vec<Shape> {
    catalog()
        .iter()
        .filter(|m| m.name.starts_with("gemini-diamond"))
        .map(|m| {
            let a = trim01(&m.alpha);
            let mx = a.max();
            let native = median(&mut a.data.iter().copied().filter(|&v| v > 0.5 * mx).collect::<Vec<_>>());
            Shape { name: m.name, s: Plane { w: a.w, h: a.h, data: a.data.iter().map(|v| v / native).collect() }, native, nsize: a.w.max(a.h) }
        })
        .collect()
}

fn resized(s: &Plane, size: usize) -> Plane {
    if size < s.h {
        resize_area(s, size, size)
    } else {
        resize_cubic(s, size, size)
    }
}

fn gray(ch: &[Plane; 3]) -> Plane {
    let (r, g, b) = (&ch[0], &ch[1], &ch[2]);
    Plane { w: r.w, h: r.h, data: (0..r.data.len()).map(|i| 0.299 * r.data[i] + 0.587 * g.data[i] + 0.114 * b.data[i]).collect() }
}

/// What detection keeps of one correlation map: its first maximum and, per generator placement, the probe value and
/// the position the reference would take (numpy argmax over the clipped 3×3 window, unravelled as 3×3).
struct Summary {
    max: f32,
    x: usize,
    y: usize,
    probes: [Option<(f32, usize, usize)>; 3],
}

fn summarise(r: &Plane, cs: usize, size: usize) -> Summary {
    let (max, x, y) = crate::ncc::max_loc(r);
    let probes = PLACE.map(|k| {
        let p = cs as i64 - size as i64 - (k * size as f64).round() as i64;
        if p < 0 || p as usize >= r.w || p as usize >= r.h {
            return None;
        }
        let (px, py) = (p as usize, p as usize);
        let (wx0, wy0) = (px.saturating_sub(1), py.saturating_sub(1));
        let (wx1, wy1) = ((px + 2).min(r.w), (py + 2).min(r.h));
        let win: Vec<(usize, usize)> = (wy0..wy1).flat_map(|y| (wx0..wx1).map(move |x| (x, y))).collect();
        let v = if py > 0 && px > 0 { win.iter().map(|&(x, y)| r.at(x, y)).fold(f32::NEG_INFINITY, f32::max) } else { r.at(px, py) };
        let (mut bi, mut bv) = (0usize, f32::NEG_INFINITY);
        for (idx, &(x, y)) in win.iter().enumerate() {
            if r.at(x, y) > bv {
                (bi, bv) = (idx, r.at(x, y));
            }
        }
        Some((v, wx0 + bi % 3, wy0 + bi / 3))
    });
    Summary { max, x, y, probes }
}

/// Detection result: (ncc, shape index, size, x, y, known placement).
fn detect(ch: &[Plane; 3], w: usize, h: usize, sh: &[Shape]) -> (f32, usize, usize, usize, usize, bool) {
    let cs = h.min(w).min(320.max(h.min(w) / 3));
    let (cx0, cy0) = (w - cs, h - cs);
    let corner: [Plane; 3] = [0, 1, 2].map(|c| ch[c].crop(cx0, cy0, cs, cs));
    let g = gray(&corner);
    let b = gaussian_blur(&g, 3.0);
    let hp = Plane { w: cs, h: cs, data: g.data.iter().zip(&b.data).map(|(a, b)| a - b).collect() };
    let plan = CorrPlan::new(&hp);
    let jobs: Vec<(usize, usize)> = (0..sh.len()).flat_map(|i| (24..(200.min(cs / 2))).step_by(2).map(move |s| (i, s))).collect();
    let tpl = |k: usize| {
        let (i, size) = jobs[k];
        let a = resized(&sh[i].s, size);
        let bl = gaussian_blur(&a, 3.0);
        Plane { w: size, h: size, data: a.data.iter().zip(&bl.data).map(|(x, y)| x - y).collect() }
    };
    // two templates per FFT; keep only what the selection needs (the global max and the placement probes)
    let pairs: Vec<Vec<Summary>> = crate::par_map(jobs.len().div_ceil(2), |p| {
        let k = 2 * p;
        if k + 1 < jobs.len() {
            let (r1, r2) = plan.match_pair(&tpl(k), &tpl(k + 1));
            vec![summarise(&r1, cs, jobs[k].1), summarise(&r2, cs, jobs[k + 1].1)]
        } else {
            vec![summarise(&plan.match_template(&tpl(k)), cs, jobs[k].1)]
        }
    });
    let mut best: Option<(f32, f32, usize, usize, usize, usize, bool)> = None;
    for (&(i, size), sm) in jobs.iter().zip(pairs.iter().flatten()) {
        let (sc, mx, my) = (sm.max, sm.x, sm.y);
        let (mr, mb) = ((cs - mx - size) as f64, (cs - my - size) as f64);
        let tol = 3f64.max(0.1 * size as f64);
        let known = PLACE.iter().any(|k| (mr - k * size as f64).abs() <= tol && (mb - k * size as f64).abs() <= tol);
        let key = sc + if known { 0.15 } else { 0.0 };
        if best.is_none_or(|b| key > b.0) {
            best = Some((key, sc, i, size, mx + cx0, my + cy0, known));
        }
        for &(v, x, y) in sm.probes.iter().flatten() {
            if v + 0.15 > best.unwrap().0 {
                best = Some((v + 0.15, v, i, size, x + cx0, y + cy0, true));
            }
        }
    }
    let b = best.expect("at least one template");
    (b.1, b.2, b.3, b.4, b.5, b.6)
}

struct Cal<'a> {
    j: &'a [Plane; 3],
    jw: usize,
    jh: usize,
    ox: usize,
    oy: usize,
    msize: usize,
}

impl Cal<'_> {
    /// Shape rendered at size msize+ds and sub-pixel offset (dx, dy) on the calibration window (cv2.warpAffine).
    fn shape_at(&self, s: &Plane, dx: f32, dy: f32, ds: f32) -> Plane {
        let base = resized(s, self.msize);
        let mut sp = Plane::new(self.jw, self.jh);
        for y in 0..self.msize {
            for x in 0..self.msize {
                sp.set(self.ox + x, self.oy + y, base.at(x, y));
            }
        }
        let k = (self.msize as f32 + ds) / self.msize as f32;
        let (tx, ty) = (self.ox as f32 * (1.0 - k) + dx, self.oy as f32 * (1.0 - k) + dy);
        let mx = Plane { w: self.jw, h: self.jh, data: (0..self.jw * self.jh).map(|i| ((i % self.jw) as f32 - tx) / k).collect() };
        let my = Plane { w: self.jw, h: self.jh, data: (0..self.jw * self.jh).map(|i| ((i / self.jw) as f32 - ty) / k).collect() };
        remap(&sp, &mx, &my, Interp::Linear, Border::Constant(0.0))
    }

    fn restore(&self, a: &Plane) -> [Plane; 3] {
        [0, 1, 2].map(|c| Plane {
            w: a.w,
            h: a.h,
            data: self.j[c].data.iter().zip(&a.data).map(|(&jv, &av)| (jv - av * 255.0) / (1.0 - av).max(0.05)).collect(),
        })
    }

    fn band(s: &Plane) -> (Plane, Plane, Vec<bool>) {
        let (sx, sy) = sobel3(s);
        let m: Vec<f32> = sx.data.iter().zip(&sy.data).map(|(a, b)| a.hypot(*b)).collect();
        let mx = m.iter().copied().fold(0.0, f32::max);
        let band = m.iter().map(|&v| v > 0.1 * mx).collect();
        (sx, sy, band)
    }

    fn energy_of(&self, s: &Plane, g: f32, band: &[bool]) -> f32 {
        let a = Plane { w: s.w, h: s.h, data: s.data.iter().map(|v| v * g).collect() };
        let img = self.restore(&a);
        let nb = band.iter().filter(|&&b| b).count().max(1) as f64;
        let mut e = 0f64;
        for c in &img {
            let (ix, iy) = sobel3(c);
            e += (0..band.len()).filter(|&i| band[i]).map(|i| (ix.data[i] * ix.data[i] + iy.data[i] * iy.data[i]) as f64).sum::<f64>() / nb;
        }
        e as f32
    }

    /// Opacity = zero crossing of the signed gradient correlation; returns (opacity, left-over edge energy).
    fn fit(&self, s: &Plane) -> (f32, f32) {
        let (sx, sy, band) = Self::band(s);
        let corr = |g: f32| -> f64 {
            let a = Plane { w: s.w, h: s.h, data: s.data.iter().map(|v| v * g).collect() };
            let mut c = 0f64;
            for ch in &self.restore(&a) {
                let (ix, iy) = sobel3(ch);
                c += (0..band.len()).filter(|&i| band[i]).map(|i| (ix.data[i] * sx.data[i] + iy.data[i] * sy.data[i]) as f64).sum::<f64>();
            }
            c
        };
        let (mut lo, mut hi, mut m) = (0.05f32, 0.9f32, 0.0);
        for _ in 0..22 {
            m = (lo + hi) / 2.0;
            if corr(m) > 0.0 {
                lo = m;
            } else {
                hi = m;
            }
        }
        (m, self.energy_of(s, m, &band))
    }

    fn energy(&self, s: &Plane, g: f32) -> f32 {
        let (_, _, band) = Self::band(s);
        self.energy_of(s, g, &band)
    }
}

/// Remove the Gemini diamond from an interleaved 8-bit RGB image. `lossy`: the input came from JPEG/WebP;
/// `lam` < 0 = automatic TV strength. Err = no mark found with confidence (image must be left untouched).
pub fn remove_still(rgb: &[u8], w: usize, h: usize, lossy: bool, lam: f32) -> Result<StillResult, Refusal> {
    let mut log = vec![];
    let ch: [Plane; 3] = [0, 1, 2].map(|c| Plane { w, h, data: (0..w * h).map(|i| rgb[i * 3 + c] as f32).collect() });
    let sh = shapes();
    let (ncc, si, msize, mx, my, known) = detect(&ch, w, h, &sh);
    let floor = if known { 0.3 } else { 0.6 };
    log.push(format!("detected {} {msize}px at ({mx},{my}) ncc={ncc:.3}{}, floor {floor}", sh[si].name, if known { " (known placement)" } else { "" }));
    if ncc < floor {
        return Err(Refusal { best_ncc: ncc, floor });
    }
    const P: usize = 6;
    let (x0, y0) = (mx.saturating_sub(P), my.saturating_sub(P));
    let (x1, y1) = ((mx + msize + P).min(w), (my + msize + P).min(h));
    let j: [Plane; 3] = [0, 1, 2].map(|c| ch[c].crop(x0, y0, x1 - x0, y1 - y0));
    let cal = Cal { j: &j, jw: x1 - x0, jh: y1 - y0, ox: mx - x0, oy: my - y0, msize };
    // shape × offset grid, each with its fitted opacity
    let grid: Vec<f32> = vec![-1.0, -0.5, 0.0, 0.5, 1.0];
    let jobs: Vec<(usize, f32, f32)> = (0..sh.len()).flat_map(|i| grid.iter().flat_map(move |&dy| [-1.0f32, -0.5, 0.0, 0.5, 1.0].map(move |dx| (i, dx, dy)))).collect();
    let fits: Vec<(f32, f32)> = crate::par_map(jobs.len(), |k| {
        let (i, dx, dy) = jobs[k];
        cal.fit(&cal.shape_at(&sh[i].s, dx, dy, 0.0))
    });
    // (energy, shape, dx, dy, opacity)
    let mut cands: Vec<(f32, usize, f32, f32, f32)> = jobs.iter().zip(&fits).map(|(&(i, dx, dy), &(g, e))| (e, i, dx, dy, g)).collect();
    let snapped: Vec<(f32, usize, f32, f32, f32)> = cands
        .iter()
        .filter(|c| (c.4 - sh[c.1].native).abs() < 0.04 * 1f32.max(sh[c.1].native / 0.3))
        .map(|c| (cal.energy(&cal.shape_at(&sh[c.1].s, c.2, c.3, 0.0), sh[c.1].native), c.1, c.2, c.3, sh[c.1].native))
        .collect();
    if !snapped.is_empty() {
        cands = snapped;
    }
    let order = |a: &(f32, usize, f32, f32, f32), b: &(f32, usize, f32, f32, f32)| {
        a.0.partial_cmp(&b.0).unwrap().then(sh[a.1].name.cmp(sh[b.1].name)).then(a.2.partial_cmp(&b.2).unwrap()).then(a.3.partial_cmp(&b.3).unwrap())
    };
    cands.sort_by(order);
    let exact: Vec<_> = cands.iter().filter(|c| (sh[c.1].nsize as i64 - msize as i64).abs() <= 1).cloned().collect();
    if !exact.is_empty() {
        cands = exact;
    }
    let (mut e0, si, cx, cy, mut gain) = cands[0];
    let (mut dx, mut dy) = (cx, cy);
    for ddy in [-0.25f32, 0.0, 0.25] {
        for ddx in [-0.25f32, 0.0, 0.25] {
            let s_ = cal.shape_at(&sh[si].s, cx + ddx, cy + ddy, 0.0);
            let f = cal.fit(&s_);
            for (gm, e) in [f, (gain, cal.energy(&s_, gain))] {
                if e < e0 {
                    (e0, gain, dx, dy) = (e, gm, cx + ddx, cy + ddy);
                }
            }
        }
    }
    let mut ds = 0f32;
    let (mut nx, mut ny) = (dx, dy);
    if ![36, 48, 96].contains(&msize) {
        for dds in [-1.0f32, -0.75, -0.5, -0.25, 0.25, 0.5, 0.75, 1.0] {
            for ddy in [-0.5f32, -0.25, 0.0, 0.25, 0.5] {
                for ddx in [-0.5f32, -0.25, 0.0, 0.25, 0.5] {
                    let s_ = cal.shape_at(&sh[si].s, dx + ddx, dy + ddy, dds);
                    let f = cal.fit(&s_);
                    for (gm, e) in [f, (gain, cal.energy(&s_, gain))] {
                        if e < e0 {
                            (e0, gain, nx, ny, ds) = (e, gm, dx + ddx, dy + ddy, dds);
                        }
                    }
                }
            }
        }
    }
    if ds != 0.0 {
        (dx, dy) = (nx, ny);
    }
    log.push(format!("calibrated: shape {}, opacity {gain:.3}, offset ({dx:+.2},{dy:+.2}), size {:.2}px", sh[si].name, msize as f32 + ds));
    // a mark that is not there calibrates to ~0 opacity (real Google marks: 0.28–0.50); never touch the image then
    if gain < MIN_OPACITY {
        return Err(Refusal { best_ncc: ncc, floor });
    }
    let s_ = cal.shape_at(&sh[si].s, dx, dy, ds);
    let a = Plane { w: s_.w, h: s_.h, data: s_.data.iter().map(|v| (v * gain).clamp(0.0, 0.95)).collect() };
    let mut img = cal.restore(&a);
    // lossy inputs: TV strength from the texture around the mark
    let on: Vec<bool> = a.data.iter().map(|&v| v > 0.01).collect();
    let ring: Vec<bool> = dilate_rect(&on, a.w, a.h, 9, 9).iter().zip(&on).map(|(&d, &o)| d && !o).collect();
    let (gx, gy) = sobel3(&gray(&j));
    let tex_v: Vec<f32> = (0..ring.len()).filter(|&i| ring[i]).map(|i| gx.data[i].hypot(gy.data[i]) / 8.0).collect();
    let tex = tex_v.iter().map(|&v| v as f64).sum::<f64>() / tex_v.len().max(1) as f64;
    let lam = if lam >= 0.0 {
        lam
    } else if lossy {
        (6.0 - 5.0 * (tex as f32 - 3.0) / 9.0).clamp(1.0, 6.0)
    } else {
        0.0
    };
    log.push(format!("texture {tex:.1} -> TV lambda {lam:.1}"));
    if lam > 0.0 {
        let wgt = Plane { w: a.w, h: a.h, data: a.data.iter().map(|v| (1.0 - v) * (1.0 - v) + 0.02).collect() };
        let mask = dilate_rect(&on, a.w, a.h, 3, 3);
        let lp = Plane::filled(a.w, a.h, lam);
        for c in img.iter_mut() {
            *c = tv_refine(c, &wgt, &mask, &lp, 200, 1.0);
        }
    }
    let mut out = rgb.to_vec();
    for y in 0..cal.jh {
        for x in 0..cal.jw {
            for c in 0..3 {
                out[((y0 + y) * w + x0 + x) * 3 + c] = (img[c].at(x, y) + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(StillResult { rgb: out, log })
}
