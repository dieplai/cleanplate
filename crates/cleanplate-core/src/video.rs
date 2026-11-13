//! Video region rebuild: research/pipeline.py lines 138–362 (opaque mode, guide, flow-guided gathering, gross-outlier
//! veto, robust fusion, weighted-TV MAP, chroma with its own veto). Flows are computed once per ordered pair and
//! cached per window of frames; frames are fused in parallel.
use crate::detect::Detection;
use crate::filter::{dilate_rect, gaussian_blur, harmonic_fill, sobel3};
use crate::flow::{dis_flow, Flow};
use crate::geom::{remap, Border, Interp};
use crate::tv::tv_refine;
use crate::Plane;
use std::collections::HashMap;

pub struct Prepared {
    pub alpha: Plane,
    pub mx: i64,
    pub my: i64,
    /// Opaque-mode dilation, −1 for a plain alpha overlay.
    pub op: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub x0: usize,
    pub y0: usize,
    pub w: usize,
    pub h: usize,
}

#[derive(Clone, Debug)]
pub struct Params {
    pub k: usize,
    /// TV strength; negative = automatic per shot (16 on static/slow shots → 0 on moving ones).
    pub lam: f32,
    pub edge_tv: bool,
    /// White in the stored range: 235 for TV range, 255 for full range.
    pub tv_range_white: f32,
}

impl Default for Params {
    fn default() -> Self {
        Params { k: 8, lam: -1.0, edge_tv: true, tv_range_white: 235.0 }
    }
}

/// Guide planes (a cleaned lower-resolution version of the same clip) already resampled onto the region grid.
pub struct Guide {
    pub y: Vec<Plane>,
    pub u: Vec<Plane>,
    pub v: Vec<Plane>,
}

#[derive(Clone, Debug)]
pub struct ShotLog {
    pub s0: usize,
    pub s1: usize,
    pub motion: f32,
    pub k: usize,
    pub lam: f32,
    pub clean_share: f32,
}

pub struct Rebuilt {
    pub y: Vec<Plane>,
    pub u: Vec<Plane>,
    pub v: Vec<Plane>,
    pub feather_y: Plane,
    pub feather_c: Plane,
    pub shot_logs: Vec<ShotLog>,
}

/// Opaque-mode decision and mark support (pipeline.py lines 138–145).
pub fn prepare_alpha(det: &Detection, opaque_flag: i32, has_guide: bool) -> Prepared {
    let mut op = if opaque_flag >= 0 { opaque_flag } else { det.mark.opaque.map(|v| v as i32).unwrap_or(-1) };
    if has_guide && opaque_flag < 0 && op >= 0 {
        op = 3; // with real guide content, also replace the upscaler's ringing/blocky halo around the strokes
    }
    let (mut a, mut mx, mut my) = (det.alpha.clone(), det.x as i64, det.y as i64);
    if op >= 0 {
        let pad = (op + 1) as usize;
        a = a.pad_const(pad, 0.0);
        mx -= pad as i64;
        my -= pad as i64;
        let k = (2 * op + 1) as usize;
        let m: Vec<bool> = a.data.iter().map(|&v| v > 0.08).collect();
        let d = dilate_rect(&m, a.w, a.h, k, k);
        a = Plane { w: a.w, h: a.h, data: d.iter().map(|&b| if b { 0.95 } else { 0.0 }).collect() };
    }
    Prepared { alpha: a, mx, my, op }
}

/// Working region: mark + 40 px of context, even-aligned, inside the corner crop (pipeline.py lines 148–151).
pub fn region_bounds(p: &Prepared, crop_w: usize, crop_h: usize) -> Region {
    const MG: i64 = 40;
    let (tw, th) = (p.alpha.w as i64, p.alpha.h as i64);
    let x0 = (p.mx - MG).max(0) & !1;
    let y0 = (p.my - MG).max(0) & !1;
    let x1 = (crop_w as i64).min((p.mx + tw + MG + 1) & !1);
    let y1 = (crop_h as i64).min((p.my + th + MG + 1) & !1);
    Region { x0: x0 as usize, y0: y0 as usize, w: (x1 - x0) as usize, h: (y1 - y0) as usize }
}

/// Alpha on the region grid, clipped to [0, 0.95].
pub fn place_alpha(p: &Prepared, r: &Region) -> Plane {
    let mut a = Plane::new(r.w, r.h);
    let (ox, oy) = (p.mx - r.x0 as i64, p.my - r.y0 as i64);
    for y in 0..p.alpha.h {
        for x in 0..p.alpha.w {
            let (xx, yy) = (ox + x as i64, oy + y as i64);
            if xx >= 0 && yy >= 0 && (xx as usize) < r.w && (yy as usize) < r.h {
                a.set(xx as usize, yy as usize, p.alpha.at(x, y).clamp(0.0, 0.95));
            }
        }
    }
    a
}

fn sub2(p: &Plane) -> Plane {
    let (w, h) = (p.w.div_ceil(2), p.h.div_ceil(2));
    Plane { w, h, data: (0..w * h).map(|k| p.at(2 * (k % w), 2 * (k / w))).collect() }
}

fn sub2_mask(m: &[bool], w: usize, h: usize) -> Vec<bool> {
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    (0..cw * ch).map(|k| m[2 * (k / cw) * w + 2 * (k % cw)]).collect()
}

/// Chroma alpha with H.264's default 4:2:0 siting ("left"): horizontal [¼ ½ ¼] at even columns, vertical pair mean.
fn chroma_alpha(ay: &Plane) -> Plane {
    let (w, h) = (ay.w, ay.h);
    let hz = |x: isize, y: usize| ay.at(x.clamp(0, w as isize - 1) as usize, y);
    let (cw, ch) = (w / 2, h / 2);
    let mut out = Plane::new(cw, ch);
    for y in 0..ch {
        for x in 0..cw {
            let f = |yy: usize| 0.25 * hz(2 * x as isize - 1, yy) + 0.5 * hz(2 * x as isize, yy) + 0.25 * hz(2 * x as isize + 1, yy);
            out.set(x, y, 0.5 * (f(2 * y) + f(2 * y + 1)));
        }
    }
    out
}

fn invert(j: &Plane, a: &Plane, l: f32) -> Plane {
    Plane { w: j.w, h: j.h, data: j.data.iter().zip(&a.data).map(|(&jv, &av)| (jv - av * l) / (1.0 - av)).collect() }
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

struct Ctx<'a> {
    w: usize,
    h: usize,
    ry: &'a [Plane],
    ru: &'a [Plane],
    rv: &'a [Plane],
    r8: &'a [Plane],
    ay: &'a Plane,
    ac: &'a Plane,
    m: &'a [bool],
    mflow_w: &'a [bool],
    fw: (usize, usize, usize, usize),
    edge: &'a [bool],
    hole_y: &'a [bool],
    op: i32,
    guided: bool,
    edge_tv: bool,
}

impl Ctx<'_> {
    /// DIS on the u8 inversions, then the flow inside the mark is re-filled from its surroundings.
    fn flow(&self, a: usize, b: usize) -> Flow {
        let mut f = dis_flow(&self.r8[a], &self.r8[b]);
        let (x0, y0, x1, y1) = self.fw;
        for p in [&mut f.u, &mut f.v] {
            let sub = harmonic_fill(&p.crop(x0, y0, x1 - x0, y1 - y0), self.mflow_w, 150);
            for y in y0..y1 {
                p.data[y * p.w + x0..y * p.w + x1].copy_from_slice(&sub.data[(y - y0) * sub.w..(y - y0 + 1) * sub.w]);
            }
        }
        f
    }

    /// One frame of the rebuild; returns (Y, U, V, clean weight, total weight).
    #[allow(clippy::too_many_arguments)]
    fn fuse(&self, t: usize, offs: &[i64], s0: usize, s1: usize, k: usize, lam: f32, flows: &HashMap<(usize, usize), Flow>) -> (Plane, Plane, Plane, f64, f64) {
        let (w, h, n) = (self.w, self.h, self.w * self.h);
        let ay = self.ay;
        let mut cy = vec![self.ry[t].clone()];
        let mut cu = vec![self.ru[t].clone()];
        let mut cv = vec![self.rv[t].clone()];
        let mut ws = vec![Plane { w, h, data: ay.data.iter().map(|a| (1.0 - a) * (1.0 - a)).collect() }];
        let mut cl: Vec<Vec<bool>> = vec![ay.data.iter().map(|&a| a < 0.02).collect()];
        let (mut clean_w, mut tot_w) = (0f64, 0f64);
        for &o in offs {
            let s = t as i64 + o;
            if o == 0 || s < s0 as i64 || s >= s1 as i64 {
                continue;
            }
            let s = s as usize;
            let (fts, fst) = (&flows[&(t, s)], &flows[&(s, t)]);
            let px = Plane { w, h, data: (0..n).map(|i| (i % w) as f32 + fts.u.data[i]).collect() };
            let py = Plane { w, h, data: (0..n).map(|i| (i / w) as f32 + fts.v.data[i]).collect() };
            let bu = remap(&fst.u, &px, &py, Interp::Linear, Border::Replicate);
            let bv = remap(&fst.v, &px, &py, Interp::Linear, Border::Replicate);
            let a_src = remap(ay, &px, &py, Interp::Linear, Border::Constant(0.0));
            let decay = (-((s as f32 - t as f32).abs()) / (2.0 * k as f32)).exp();
            let wt: Vec<f32> = (0..n)
                .map(|i| {
                    let fb = (fts.u.data[i] + bu.data[i]).hypot(fts.v.data[i] + bv.data[i]);
                    let inb = px.data[i] >= 0.0 && px.data[i] <= (w - 1) as f32 && py.data[i] >= 0.0 && py.data[i] <= (h - 1) as f32;
                    (1.0 - a_src.data[i]).powi(2) * (-(fb / 0.7).powi(2)).exp() * if inb { 1.0 } else { 0.0 } * decay
                })
                .collect();
            cy.push(remap(&self.ry[s], &px, &py, Interp::Cubic, Border::Replicate));
            let (hx, hy) = (sub2(&px), sub2(&py));
            let hx = Plane { w: hx.w, h: hx.h, data: hx.data.iter().map(|v| v / 2.0).collect() };
            let hy = Plane { w: hy.w, h: hy.h, data: hy.data.iter().map(|v| v / 2.0).collect() };
            cu.push(remap(&self.ru[s], &hx, &hy, Interp::Linear, Border::Replicate));
            cv.push(remap(&self.rv[s], &hx, &hy, Interp::Linear, Border::Replicate));
            let c: Vec<bool> = a_src.data.iter().map(|&a| a < 0.02).collect();
            for i in 0..n {
                if ay.data[i] > 0.05 {
                    if c[i] {
                        clean_w += wt[i] as f64;
                    }
                    tot_w += wt[i] as f64;
                }
            }
            ws.push(Plane { w, h, data: wt });
            cl.push(c);
        }
        let nc = cy.len();
        // luma: gross-outlier veto against this frame's own inversion, robust weighted mean, confidence
        let mut wt_after = ws.clone();
        let (mut mu, mut conf, mut clean_sum) = (Plane::new(w, h), Plane::new(w, h), vec![0f32; n]);
        let mut dev = vec![0f32; nc];
        for i in 0..n {
            let a = ay.data[i];
            let tol = 25.0 + 25.0 * a / (1.0 - a);
            let r0 = self.ry[t].data[i];
            for c in 1..nc {
                wt_after[c].data[i] *= (-((cy[c].data[i] - r0) / tol).powi(4)).exp();
            }
            let wsum = |wv: &dyn Fn(usize) -> f32| -> f32 {
                let (mut num, mut den) = (0f32, 0f32);
                for c in 0..nc {
                    num += wv(c) * cy[c].data[i];
                    den += wv(c);
                }
                num / (den + 1e-6)
            };
            let mut m = wsum(&|c| wt_after[c].data[i]);
            let mut wr = vec![0f32; nc];
            for _ in 0..2 {
                for c in 0..nc {
                    dev[c] = (cy[c].data[i] - m).abs();
                }
                let sc = median(&mut dev.clone()) * 1.4826 + 2.0;
                for c in 0..nc {
                    wr[c] = wt_after[c].data[i] * (1.5 * sc / (dev[c] + 1e-6)).min(1.0);
                }
                m = wsum(&|c| wr[c]);
            }
            mu.data[i] = m;
            let csum: f32 = (0..nc).filter(|&c| cl[c][i]).map(|c| wr[c]).sum();
            conf.data[i] = csum + 1.0 / (1.0 + 9.0 * (a / (1.0 - a)).powi(2));
            clean_sum[i] = csum - if a < 0.02 { wr[0] } else { 0.0 };
        }
        let lam_px = Plane {
            w,
            h,
            data: (0..n)
                .map(|i| {
                    if self.op >= 0 {
                        if !self.guided && self.hole_y[i] && clean_sum[i] < 0.5 {
                            16.0
                        } else {
                            0.0
                        }
                    } else if self.edge_tv {
                        lam.max(if self.edge[i] && clean_sum[i] < 0.5 { 16.0 } else { 0.0 })
                    } else {
                        lam
                    }
                })
                .collect(),
        };
        let oy = if lam_px.max() > 0.0 {
            tv_refine(&mu, &conf, self.m, &lam_px, 300, 1.0)
        } else {
            Plane { w, h, data: (0..n).map(|i| if self.m[i] { mu.data[i] } else { self.ry[t].data[i] }).collect() }
        };
        // chroma: same geometry weights, its OWN veto and robust step
        let (cw, ch) = (self.ac.w, self.ac.h);
        let mc = sub2_mask(self.m, w, h);
        let confc = sub2(&conf);
        let lam_c = if self.op >= 0 { sub2(&lam_px) } else { Plane::filled(cw, ch, lam) };
        let wg: Vec<Plane> = wt_after.iter().map(sub2).collect();
        let mut outs = vec![];
        for (cands, rc) in [(&cu, &self.ru[t]), (&cv, &self.rv[t])] {
            let mut muc = Plane::new(cw, ch);
            for i in 0..cw * ch {
                let a = self.ac.data[i];
                let tol = 20.0 + 20.0 * a / (1.0 - a);
                let wc: Vec<f32> = (0..nc)
                    .map(|c| if c == 0 { wg[0].data[i] } else { wg[c].data[i] * (-((cands[c].data[i] - rc.data[i]) / tol).powi(4)).exp() })
                    .collect();
                let mean = |wv: &[f32]| -> f32 {
                    let (mut num, mut den) = (0f32, 0f32);
                    for c in 0..nc {
                        num += wv[c] * cands[c].data[i];
                        den += wv[c];
                    }
                    num / (den + 1e-6)
                };
                let mut m = mean(&wc);
                for _ in 0..2 {
                    let mut d: Vec<f32> = (0..nc).map(|c| (cands[c].data[i] - m).abs()).collect();
                    let sc = median(&mut d.clone()) * 1.4826 + 1.5;
                    for c in 0..nc {
                        d[c] = wc[c] * (1.5 * sc / (d[c] + 1e-6)).min(1.0);
                    }
                    m = mean(&d);
                }
                muc.data[i] = m;
            }
            outs.push(if lam_c.max() > 0.0 {
                tv_refine(&muc, &confc, &mc, &lam_c, 300, 1.0)
            } else {
                Plane { w: cw, h: ch, data: (0..cw * ch).map(|i| if mc[i] { muc.data[i] } else { rc.data[i] }).collect() }
            });
        }
        let ov = outs.pop().unwrap();
        let ou = outs.pop().unwrap();
        (oy, ou, ov, clean_w, tot_w)
    }
}

/// Rebuild the region of every frame. `jy/ju/jv`: region crops of the decoded frames (u8 values);
/// `alpha_y`: [`place_alpha`]; `op`: [`Prepared::op`]; `shots`: [`crate::detect::shot_cuts`].
#[allow(clippy::too_many_arguments)]
pub fn rebuild(jy: &[Plane], ju: &[Plane], jv: &[Plane], alpha_y: &Plane, op: i32, shots: &[(usize, usize)], params: &Params, guide: Option<&Guide>) -> Rebuilt {
    let (w, h, n) = (alpha_y.w, alpha_y.h, jy.len());
    let mut ay = alpha_y.clone();
    let mut ac = chroma_alpha(&ay);
    let (ly, lc) = (params.tv_range_white, 128.0);
    let mut ry: Vec<Plane> = jy.iter().map(|j| invert(j, &ay, ly)).collect();
    let mut ru: Vec<Plane> = ju.iter().map(|j| invert(j, &ac, lc)).collect();
    let mut rv: Vec<Plane> = jv.iter().map(|j| invert(j, &ac, lc)).collect();
    let m = dilate_rect(&ay.data.iter().map(|&a| a > 0.01).collect::<Vec<_>>(), w, h, 5, 5);
    let mflow = dilate_rect(&m, w, h, 5, 5);
    let (mut fx0, mut fy0, mut fx1, mut fy1) = (w, h, 0, 0);
    for i in 0..w * h {
        if mflow[i] {
            let (x, y) = (i % w, i / w);
            (fx0, fy0, fx1, fy1) = (fx0.min(x), fy0.min(y), fx1.max(x), fy1.max(y));
        }
    }
    let fw = (fx0.saturating_sub(6), fy0.saturating_sub(6), (fx1 + 7).min(w), (fy1 + 7).min(h));
    let mflow_w: Vec<bool> = (fw.1..fw.3).flat_map(|y| (fw.0..fw.2).map(move |x| (x, y))).map(|(x, y)| mflow[y * w + x]).collect();
    let hole_y: Vec<bool> = ay.data.iter().map(|&a| a > 0.5).collect();
    let mut guided = false;
    if op >= 0 {
        let hole_c: Vec<bool> = ac.data.iter().map(|&a| a > 0.3).collect();
        ry = crate::par_map(n, |t| harmonic_fill(&jy[t], &hole_y, 150));
        ru = crate::par_map(n, |t| harmonic_fill(&ju[t], &hole_c, 150));
        rv = crate::par_map(n, |t| harmonic_fill(&jv[t], &hole_c, 150));
        if let Some(g) = guide {
            guided = true;
            let near = dilate_rect(&ay.data.iter().map(|&a| a > 0.01).collect::<Vec<_>>(), w, h, 3, 3);
            let rg_y: Vec<bool> = dilate_rect(&hole_y, w, h, 7, 7).iter().zip(&near).map(|(&a, &b)| a && !b).collect();
            let rg_c: Vec<bool> = sub2_mask(&rg_y, w, h).iter().zip(&hole_c).map(|(&a, &b)| a && !b).collect();
            for t in 0..n {
                let k = t.min(g.y.len() - 1);
                for (r, j, gp, hole, rg) in [(&mut ry[t], &jy[t], &g.y[k], &hole_y, &rg_y), (&mut ru[t], &ju[t], &g.u[k], &hole_c, &rg_c), (&mut rv[t], &jv[t], &g.v[k], &hole_c, &rg_c)] {
                    let mut diffs: Vec<f32> = (0..j.data.len()).filter(|&i| rg[i]).map(|i| j.data[i] - gp.data[i]).collect();
                    let off = if diffs.is_empty() { 0.0 } else { median(&mut diffs) };
                    for i in 0..j.data.len() {
                        if hole[i] {
                            r.data[i] = gp.data[i] + off;
                        }
                    }
                }
            }
            // guide-filled strokes count as a half-weight clean observation
            ay = Plane { w, h, data: hole_y.iter().map(|&b| if b { 0.29 } else { 0.0 }).collect() };
            ac = Plane { w: ac.w, h: ac.h, data: hole_c.iter().map(|&b| if b { 0.29 } else { 0.0 }).collect() };
        }
    }
    let r8: Vec<Plane> = ry.iter().map(|r| Plane { w, h, data: r.data.iter().map(|v| v.clamp(0.0, 255.0).floor()).collect() }).collect();
    let (gx, gy) = sobel3(&ay);
    let ga: Vec<f32> = gx.data.iter().zip(&gy.data).map(|(a, b)| a.hypot(*b)).collect();
    let gmax = ga.iter().copied().fold(0.0, f32::max);
    let edge = dilate_rect(&ga.iter().map(|&g| g > 0.25 * gmax).collect::<Vec<_>>(), w, h, 3, 3);
    let ring: Vec<bool> = dilate_rect(&mflow, w, h, 9, 9).iter().zip(&mflow).map(|(&a, &b)| a && !b).collect();
    let ctx = Ctx { w, h, ry: &ry, ru: &ru, rv: &rv, r8: &r8, ay: &ay, ac: &ac, m: &m, mflow_w: &mflow_w, fw, edge: &edge, hole_y: &hole_y, op, guided, edge_tv: params.edge_tv };

    let (mut oy, mut ou, mut ov) = (ry.clone(), ru.clone(), rv.clone());
    let mut logs = vec![];
    for &(s0, s1) in shots {
        let len = s1 - s0;
        let step = (len / 6).max(1);
        let probes: Vec<(usize, usize)> = (s0..s1.saturating_sub(2)).step_by(step).take(6).map(|t| (t, (t + 2).min(s1 - 1))).collect();
        let motion = if probes.is_empty() {
            0.0
        } else {
            let mut per: Vec<f32> = crate::par_map(probes.len(), |i| {
                let f = ctx.flow(probes[i].0, probes[i].1);
                let mut mags: Vec<f32> = (0..w * h).filter(|&k| ring[k]).map(|k| f.u.data[k].hypot(f.v.data[k])).collect();
                median(&mut mags)
            });
            median(&mut per)
        };
        let (k, offs): (usize, Vec<i64>) = if motion > 0.3 {
            (params.k, (-(params.k as i64)..=params.k as i64).collect())
        } else {
            let k = (3 * params.k).min(len);
            let offs = if op >= 0 && guided { vec![0] } else { (-(k as i64)..=k as i64).collect() };
            (k, offs)
        };
        let lam = if params.lam >= 0.0 { params.lam } else { 16.0 * ((1.2 - motion) / 0.6).clamp(0.0, 1.0) };
        let (mut clean_w, mut tot_w) = (0f64, 0f64);
        let mut cache: HashMap<(usize, usize), Flow> = HashMap::new();
        const CHUNK: usize = 32;
        let mut c0 = s0;
        while c0 < s1 {
            let c1 = (c0 + CHUNK).min(s1);
            let mut need: Vec<(usize, usize)> = vec![];
            for t in c0..c1 {
                for &o in &offs {
                    let s = t as i64 + o;
                    if o != 0 && s >= s0 as i64 && s < s1 as i64 {
                        for pr in [(t, s as usize), (s as usize, t)] {
                            if !cache.contains_key(&pr) && !need.contains(&pr) {
                                need.push(pr);
                            }
                        }
                    }
                }
            }
            let fresh = crate::par_map(need.len(), |i| ctx.flow(need[i].0, need[i].1));
            cache.extend(need.into_iter().zip(fresh));
            let frames = crate::par_map(c1 - c0, |i| ctx.fuse(c0 + i, &offs, s0, s1, k, lam, &cache));
            for (i, (fy, fu, fv, cw, tw)) in frames.into_iter().enumerate() {
                oy[c0 + i] = fy;
                ou[c0 + i] = fu;
                ov[c0 + i] = fv;
                clean_w += cw;
                tot_w += tw;
            }
            let keep_from = c1.saturating_sub(k);
            cache.retain(|&(a, b), _| a.min(b) >= keep_from);
            c0 = c1;
        }
        logs.push(ShotLog { s0, s1, motion, k, lam, clean_share: (clean_w / tot_w.max(1e-6)) as f32 });
    }
    let mf = Plane { w, h, data: m.iter().map(|&b| if b { 1.0 } else { 0.0 }).collect() };
    let fy = gaussian_blur(&mf, 1.0);
    let feather_y = Plane { w, h, data: fy.data.iter().map(|v| (v * 1.5).clamp(0.0, 1.0)).collect() };
    let feather_c = sub2(&feather_y);
    Rebuilt { y: oy, u: ou, v: ov, feather_y, feather_c, shot_logs: logs }
}
