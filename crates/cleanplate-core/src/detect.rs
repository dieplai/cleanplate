//! Safe video detection and hard-cut shot detection (research/pipeline.py, sections "detect" and "shots").
use crate::filter::gaussian_blur;
use crate::geom::{resize_area_fx, resize_cubic_fx};
use crate::marks::{by_name, catalog, Mark};
use crate::ncc::{match_template_ccoeff_normed, max_loc};
use crate::Plane;

#[derive(Debug)]
pub struct Detection {
    pub mark: &'static Mark,
    pub scale: f32,
    /// Top-left of the (scaled) mark inside the corner crop.
    pub x: usize,
    pub y: usize,
    pub ncc: f32,
    pub at_known: bool,
    /// The scaled alpha that was matched.
    pub alpha: Plane,
}

#[derive(Debug)]
pub struct Refusal {
    pub best_ncc: f32,
    pub floor: f32,
}

/// Mean over sampled frames of (frame − GaussianBlur σ 3): the mark is the only static high-frequency structure.
fn hp_mean(corners: &[&Plane]) -> Plane {
    let step = (corners.len() / 120).max(1);
    let samp: Vec<&&Plane> = corners.iter().step_by(step).collect();
    let parts: Vec<Plane> = crate::par_map(samp.len(), |i| {
        let f = samp[i];
        let b = gaussian_blur(f, 3.0);
        Plane { w: f.w, h: f.h, data: f.data.iter().zip(&b.data).map(|(a, b)| a - b).collect() }
    });
    let mut acc = vec![0f64; corners[0].w * corners[0].h];
    for p in &parts {
        for (a, &v) in acc.iter_mut().zip(&p.data) {
            *a += v as f64;
        }
    }
    let n = parts.len() as f64;
    Plane { w: corners[0].w, h: corners[0].h, data: acc.iter().map(|&v| (v / n) as f32).collect() }
}

/// Find the generator mark in the bottom-right corner crops (all frames, luma, u8-valued planes).
/// Refuses (Err) below 0.45 at a known placement, below max(min_ncc, 0.75) elsewhere.
pub fn detect_video(corners: &[&Plane], frame_w: usize, frame_h: usize, min_ncc: f32) -> Result<Detection, Refusal> {
    let (cw, ch) = (corners[0].w, corners[0].h);
    let hp = hp_mean(corners);
    let sc2 = ((frame_w.min(frame_h) as f64 / 720.0) * 1000.0).round() / 1000.0;
    let mut scales = vec![1.0f64];
    if sc2 != 1.0 {
        scales.push(sc2);
        scales.sort_by(|a, b| a.partial_cmp(b).unwrap());
    }
    let mut best: Option<(f32, &'static Mark, f64, usize, usize, Plane)> = None;
    for m in catalog() {
        for &sc in &scales {
            let a = if sc == 1.0 {
                m.alpha.clone()
            } else if sc < 1.0 {
                resize_area_fx(&m.alpha, sc)
            } else {
                resize_cubic_fx(&m.alpha, sc)
            };
            if a.h >= ch || a.w >= cw || a.w.min(a.h) < 6 {
                continue;
            }
            let b = gaussian_blur(&a, 3.0);
            let t = Plane { w: a.w, h: a.h, data: a.data.iter().zip(&b.data).map(|(x, y)| x - y).collect() };
            let (v, x, y) = max_loc(&match_template_ccoeff_normed(&hp, &t));
            if best.as_ref().is_none_or(|b| v > b.0) {
                best = Some((v, m, sc, x, y, a));
            }
        }
    }
    let (ncc, mark, sc, mx, my, a) = best.expect("at least one template fits the corner");
    let (mr, mb) = (cw as f64 - (mx + a.w) as f64, ch as f64 - (my + a.h) as f64);
    let at_known = mark.known.iter().any(|&(r, b)| (mr - r as f64 * sc).abs() <= 3.0 && (mb - b as f64 * sc).abs() <= 3.0);
    let floor = if at_known { 0.45 } else { min_ncc.max(0.75) };
    if ncc < floor {
        return Err(Refusal { best_ncc: ncc, floor });
    }
    // Flow 1080p: the re-drawn mark can match the 1.5x 720p glyph better on bright backgrounds; at its known placement it
    // is the same mark, so use the native 1080p support.
    if mark.name == "veo-small-720p.png" && (sc - 1.5).abs() < 0.01 && at_known {
        let m = by_name("veo-small-1080p.png");
        let a = m.alpha.clone();
        return Ok(Detection { mark: m, scale: 1.0, x: cw - 24 - a.w, y: ch - 24 - a.h, ncc, at_known, alpha: a });
    }
    Ok(Detection { mark, scale: sc as f32, x: mx, y: my, ncc, at_known, alpha: a })
}

fn median(v: &mut [f32]) -> f32 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// Hard cuts from 64×36 luma thumbnails: d_i = mean |thumb_i − thumb_{i−1}|; a cut where
/// d_i > max(10, 5·(median(d[i−12 .. i+12]) + 1)) and at least 4 frames after the previous cut.
pub fn shot_cuts(thumbs: &[Plane]) -> Vec<(usize, usize)> {
    let n = thumbs.len();
    let mut d = vec![0f32; n];
    for i in 1..n {
        let s: f64 = thumbs[i].data.iter().zip(&thumbs[i - 1].data).map(|(a, b)| (a - b).abs() as f64).sum();
        d[i] = (s / thumbs[i].data.len() as f64) as f32;
    }
    let mut cuts = vec![0usize];
    for i in 1..n {
        let local = median(&mut d[(i.saturating_sub(12)).max(1)..(i + 12).min(n)].to_vec()) + 1.0;
        if d[i] > (5.0 * local).max(10.0) && i - cuts.last().unwrap() >= 4 {
            cuts.push(i);
        }
    }
    let mut ends: Vec<usize> = cuts[1..].to_vec();
    ends.push(n);
    cuts.into_iter().zip(ends).collect()
}
