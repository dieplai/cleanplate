//! Dense Inverse Search optical flow (Kroeger et al., ECCV 2016) with OpenCV's PRESET_MEDIUM parameters, following
//! the structure of OpenCV `dis_flow.cpp`: coarse-to-fine; per scale an inverse search of patches on a stride grid
//! (spatial propagation, Gauss–Newton with mean normalisation), densification, variational refinement; the finest
//! computed scale is 1 (half resolution) and the result is upscaled ×2.
use crate::filter::sobel3;
use crate::geom::{remap, resize_area, resize_linear, Border, Interp};
use crate::Plane;

pub struct Flow {
    pub u: Plane,
    pub v: Plane,
}

const PSZ: usize = 8;
const STRIDE: usize = 3;
const GD_ITER: usize = 25;
const VR_FIXED_POINT: usize = 5;
const VR_SOR: usize = 5;
const VR_OMEGA: f32 = 1.6;
const VR_ALPHA: f32 = 20.0;
const VR_DELTA: f32 = 5.0;
const VR_GAMMA: f32 = 10.0;
const FINEST: usize = 1;
const BSZ: usize = 16;
const EPS_DET: f32 = 0.001;
/// OpenCV splits the patch rows into `getNumThreads()` stripes (12 on the reference machine) and never propagates
/// flow vertically across a stripe boundary; fixed here so results do not depend on our own thread count.
const NUM_STRIPES: usize = 12;

fn round_u8(p: &Plane) -> Plane {
    Plane { w: p.w, h: p.h, data: p.data.iter().map(|v| v.round().clamp(0.0, 255.0)).collect() }
}

/// I1 with a `BSZ` replicated border.
fn extend(p: &Plane) -> Plane {
    let (w, h) = (p.w + 2 * BSZ, p.h + 2 * BSZ);
    let mut out = Plane::new(w, h);
    for y in 0..h {
        let sy = (y as isize - BSZ as isize).clamp(0, p.h as isize - 1) as usize;
        for x in 0..w {
            let sx = (x as isize - BSZ as isize).clamp(0, p.w as isize - 1) as usize;
            out.data[y * w + x] = p.at(sx, sy);
        }
    }
    out
}

/// Bilinear sample position of patch/pixel (i, j) displaced by (ux, uy) inside I1_ext, clamped like OpenCV.
#[inline]
fn sample_pos(i: usize, j: usize, ux: f32, uy: f32, w: usize, h: usize) -> (usize, usize, [f32; 4]) {
    let (lo_i, hi_i) = ((BSZ - PSZ + 1) as f32, (BSZ + h - 1) as f32);
    let (lo_j, hi_j) = ((BSZ - PSZ + 1) as f32, (BSZ + w - 1) as f32);
    let ii = (i as f32 + uy + BSZ as f32).clamp(lo_i, hi_i);
    let jj = (j as f32 + ux + BSZ as f32).clamp(lo_j, hi_j);
    let (di, dj) = (ii - ii.floor(), jj - jj.floor());
    (ii as usize, jj as usize, [(1.0 - di) * (1.0 - dj), (1.0 - di) * dj, di * (1.0 - dj), di * dj])
}

#[inline]
fn bilinear(e: &Plane, r: usize, c: usize, wt: &[f32; 4]) -> f32 {
    let i = r * e.w + c;
    wt[0] * e.data[i] + wt[1] * e.data[i + 1] + wt[2] * e.data[i + e.w] + wt[3] * e.data[i + e.w + 1]
}

struct Scale {
    i0: Plane,
    i1: Plane,
    i1e: Plane,
    ix: Plane,
    iy: Plane,
}

/// Mean-normalised SSD of the patch at (i, j) displaced by (ux, uy).
fn ssd(s: &Scale, i: usize, j: usize, ux: f32, uy: f32) -> f32 {
    let (r, c, wt) = sample_pos(i, j, ux, uy, s.i0.w, s.i0.h);
    let (mut sd, mut sd2) = (0f32, 0f32);
    for p in 0..PSZ {
        for q in 0..PSZ {
            let d = bilinear(&s.i1e, r + p, c + q, &wt) - s.i0.at(j + q, i + p);
            sd += d;
            sd2 += d * d;
        }
    }
    sd2 - sd * sd / (PSZ * PSZ) as f32
}

struct PatchStats {
    inv: [f32; 3], // invH11, invH12, invH22 (mean-normalised Hessian)
    gx: f32,
    gy: f32,
}

fn patch_stats(s: &Scale, i: usize, j: usize) -> PatchStats {
    let (mut xx, mut yy, mut xy, mut gx, mut gy) = (0f32, 0f32, 0f32, 0f32, 0f32);
    for p in 0..PSZ {
        for q in 0..PSZ {
            let (a, b) = (s.ix.at(j + q, i + p), s.iy.at(j + q, i + p));
            xx += a * a;
            yy += b * b;
            xy += a * b;
            gx += a;
            gy += b;
        }
    }
    let n = (PSZ * PSZ) as f32;
    let (xx, yy, xy) = (xx - gx * gx / n, yy - gy * gy / n, xy - gx * gy / n);
    let mut det = xx * yy - xy * xy;
    if det.abs() < EPS_DET {
        det = EPS_DET;
    }
    PatchStats { inv: [yy / det, -xy / det, xx / det], gx, gy }
}

/// Sparse patch flows (hs × ws) by inverse search, initialised from the dense flow (u, v) of this scale.
fn inverse_search(s: &Scale, u: &Plane, v: &Plane) -> (Vec<f32>, Vec<f32>, usize, usize) {
    let (w, h) = (s.i0.w, s.i0.h);
    let (ws, hs) = (1 + (w - PSZ) / STRIDE, 1 + (h - PSZ) / STRIDE);
    let half = PSZ / 2;
    let stats: Vec<PatchStats> = (0..hs * ws).map(|k| patch_stats(s, (k / ws) * STRIDE, (k % ws) * STRIDE)).collect();
    let (mut sx, mut sy) = (vec![0f32; hs * ws], vec![0f32; hs * ws]);
    for k in 0..hs * ws {
        let (i, j) = ((k / ws) * STRIDE, (k % ws) * STRIDE);
        sx[k] = u.at(j + half, i + half);
        sy[k] = v.at(j + half, i + half);
    }
    let inner = GD_ITER / 2;
    let n = (PSZ * PSZ) as f32;
    let stripe = hs.div_ceil(NUM_STRIPES);
    for s0 in (0..hs).step_by(stripe) {
        let s1 = (s0 + stripe).min(hs);
        for pass in 0..2 {
            let dir: isize = if pass == 0 { 1 } else { -1 };
            let order: Vec<usize> = if dir == 1 { (s0 * ws..s1 * ws).collect() } else { (s0 * ws..s1 * ws).rev().collect() };
            for k in order {
                let (is, js) = (k / ws, k % ws);
                let (i, j) = (is * STRIDE, js * STRIDE);
                let mut best = ssd(s, i, j, sx[k], sy[k]);
                let first_col = if dir == 1 { js == 0 } else { js == ws - 1 };
                let first_row = if dir == 1 { is == s0 } else { is == s1 - 1 };
                if !first_col {
                    let kn = (k as isize - dir) as usize;
                    let c = ssd(s, i, j, sx[kn], sy[kn]);
                    if c < best {
                        best = c;
                        sx[k] = sx[kn];
                        sy[k] = sy[kn];
                    }
                }
                if !first_row {
                    let kn = (k as isize - dir * ws as isize) as usize;
                    let c = ssd(s, i, j, sx[kn], sy[kn]);
                    if c < best {
                        sx[k] = sx[kn];
                        sy[k] = sy[kn];
                    }
                }
                let st = &stats[k];
                let (mut cx, mut cy) = (sx[k], sy[k]);
                let mut prev = f32::INFINITY;
                for _ in 0..inner {
                    let (r, c, wt) = sample_pos(i, j, cx, cy, w, h);
                    let (mut sd, mut sd2, mut dux, mut duy) = (0f32, 0f32, 0f32, 0f32);
                    for p in 0..PSZ {
                        for q in 0..PSZ {
                            let d = bilinear(&s.i1e, r + p, c + q, &wt) - s.i0.at(j + q, i + p);
                            sd += d;
                            sd2 += d * d;
                            dux += d * s.ix.at(j + q, i + p);
                            duy += d * s.iy.at(j + q, i + p);
                        }
                    }
                    dux -= sd * st.gx / n;
                    duy -= sd * st.gy / n;
                    let cur = sd2 - sd * sd / n;
                    cx -= st.inv[0] * dux + st.inv[1] * duy;
                    cy -= st.inv[1] * dux + st.inv[2] * duy;
                    if cur >= prev {
                        break;
                    }
                    prev = cur;
                }
                if (cx - sx[k]).hypot(cy - sy[k]) <= PSZ as f32 {
                    sx[k] = cx;
                    sy[k] = cy;
                }
            }
        }
    }
    (sx, sy, ws, hs)
}

/// Dense flow: every pixel averages the flows of the patches covering it, weighted by 1 / max(1, |photometric error|).
fn densify(s: &Scale, sx: &[f32], sy: &[f32], ws: usize, hs: usize) -> (Plane, Plane) {
    let (w, h) = (s.i0.w, s.i0.h);
    let (mut u, mut v) = (Plane::new(w, h), Plane::new(w, h));
    let range = |p: usize, n: usize| -> (usize, usize) {
        let end = (p / STRIDE).min(n - 1);
        let start = ((p as isize - PSZ as isize + STRIDE as isize) / STRIDE as isize).max(0) as usize;
        (start.min(end), end)
    };
    for i in 0..h {
        let (is0, is1) = range(i, hs);
        for j in 0..w {
            let (js0, js1) = range(j, ws);
            let (mut su, mut sv, mut sw) = (0f32, 0f32, 0f32);
            for is in is0..=is1 {
                for js in js0..=js1 {
                    let k = is * ws + js;
                    let (r, c, wt) = sample_pos(i, j, sx[k], sy[k], w, h);
                    let d = bilinear(&s.i1e, r, c, &wt) - s.i0.at(j, i);
                    let coef = 1.0 / d.abs().max(1.0);
                    su += coef * sx[k];
                    sv += coef * sy[k];
                    sw += coef;
                }
            }
            u.set(j, i, su / sw);
            v.set(j, i, sv / sw);
        }
    }
    (u, v)
}

/// 5-point derivative [1, −8, 0, 8, −1] / 12 along x (axis 0) or y (axis 1), replicated border.
fn deriv(p: &Plane, axis: usize) -> Plane {
    let mut out = Plane::new(p.w, p.h);
    let k = [1.0 / 12.0, -8.0 / 12.0, 0.0, 8.0 / 12.0, -1.0 / 12.0];
    for y in 0..p.h {
        for x in 0..p.w {
            let mut s = 0.0;
            for (t, kk) in k.iter().enumerate() {
                let o = t as isize - 2;
                let (xx, yy) = if axis == 0 { (x as isize + o, y as isize) } else { (x as isize, y as isize + o) };
                s += kk * p.at(xx.clamp(0, p.w as isize - 1) as usize, yy.clamp(0, p.h as isize - 1) as usize);
            }
            out.data[y * p.w + x] = s;
        }
    }
    out
}

/// Variational refinement (brightness + gradient constancy, robust penaliser, TV-like smoothness), solved by
/// fixed-point iterations of red–black SOR on the increments (du, dv).
fn variational_refine(i0: &Plane, i1: &Plane, u: &mut Plane, v: &mut Plane) {
    let (w, h) = (i0.w, i0.h);
    // intensities in [0, 1] (the robust penaliser's ε = 0.001 and the α/δ/γ weights are tuned for that range)
    let n01 = |p: &Plane| Plane { w: p.w, h: p.h, data: p.data.iter().map(|x| x / 255.0).collect() };
    let (i0, i1) = (&n01(i0), &n01(i1));
    const EPS2: f32 = 0.001 * 0.001;
    let mx = Plane { w, h, data: (0..w * h).map(|k| (k % w) as f32 + u.data[k]).collect() };
    let my = Plane { w, h, data: (0..w * h).map(|k| (k / w) as f32 + v.data[k]).collect() };
    let iw = remap(i1, &mx, &my, Interp::Linear, Border::Replicate);
    let avg = Plane { w, h, data: i0.data.iter().zip(&iw.data).map(|(a, b)| 0.5 * (a + b)).collect() };
    let iz = Plane { w, h, data: iw.data.iter().zip(&i0.data).map(|(a, b)| a - b).collect() };
    let (ix, iy) = (deriv(&avg, 0), deriv(&avg, 1));
    let (ixx, ixy, iyy) = (deriv(&ix, 0), deriv(&ix, 1), deriv(&iy, 1));
    let (ixz, iyz) = (deriv(&iz, 0), deriv(&iz, 1));
    let (mut du, mut dv) = (vec![0f32; w * h], vec![0f32; w * h]);
    let nb = |k: usize| -> [Option<usize>; 4] {
        let (x, y) = (k % w, k / w);
        [(x > 0).then(|| k - 1), (x + 1 < w).then(|| k + 1), (y > 0).then(|| k - w), (y + 1 < h).then(|| k + w)]
    };
    for _ in 0..VR_FIXED_POINT {
        // robust data weights and linear system coefficients at the current increments
        let (mut a11, mut a12, mut a22, mut b1, mut b2) = (vec![0f32; w * h], vec![0f32; w * h], vec![0f32; w * h], vec![0f32; w * h], vec![0f32; w * h]);
        for k in 0..w * h {
            let (x, y, z) = (ix.data[k], iy.data[k], iz.data[k]);
            let (xx, xy, yy, xz, yz) = (ixx.data[k], ixy.data[k], iyy.data[k], ixz.data[k], iyz.data[k]);
            let zl = z + x * du[k] + y * dv[k];
            let pd = VR_DELTA / (2.0 * (zl * zl + EPS2).sqrt());
            let (gxl, gyl) = (xz + xx * du[k] + xy * dv[k], yz + xy * du[k] + yy * dv[k]);
            let pg = VR_GAMMA / (2.0 * (gxl * gxl + gyl * gyl + EPS2).sqrt());
            a11[k] = pd * x * x + pg * (xx * xx + xy * xy);
            a12[k] = pd * x * y + pg * (xx * xy + xy * yy);
            a22[k] = pd * y * y + pg * (xy * xy + yy * yy);
            b1[k] = -pd * x * z - pg * (xx * xz + xy * yz);
            b2[k] = -pd * y * z - pg * (xy * xz + yy * yz);
        }
        // smoothness weights on the full flow (u + du, v + dv)
        let fu: Vec<f32> = (0..w * h).map(|k| u.data[k] + du[k]).collect();
        let fv: Vec<f32> = (0..w * h).map(|k| v.data[k] + dv[k]).collect();
        let ps: Vec<f32> = (0..w * h)
            .map(|k| {
                let (x, y) = (k % w, k / w);
                let c = |f: &[f32], dx: isize, dy: isize| {
                    let xx = (x as isize + dx).clamp(0, w as isize - 1) as usize;
                    let yy = (y as isize + dy).clamp(0, h as isize - 1) as usize;
                    f[yy * w + xx]
                };
                let (ux, uy) = ((c(&fu, 1, 0) - c(&fu, -1, 0)) * 0.5, (c(&fu, 0, 1) - c(&fu, 0, -1)) * 0.5);
                let (vx, vy) = ((c(&fv, 1, 0) - c(&fv, -1, 0)) * 0.5, (c(&fv, 0, 1) - c(&fv, 0, -1)) * 0.5);
                VR_ALPHA / (2.0 * (ux * ux + uy * uy + vx * vx + vy * vy + EPS2).sqrt())
            })
            .collect();
        for _ in 0..VR_SOR {
            for color in 0..2 {
                for k in 0..w * h {
                    if ((k % w) + (k / w)) % 2 != color {
                        continue;
                    }
                    let (mut sw, mut su, mut sv) = (0f32, 0f32, 0f32);
                    for n in nb(k).into_iter().flatten() {
                        let wn = 0.5 * (ps[k] + ps[n]);
                        sw += wn;
                        su += wn * (u.data[n] + du[n] - u.data[k]);
                        sv += wn * (v.data[n] + dv[n] - v.data[k]);
                    }
                    let nu = (b1[k] - a12[k] * dv[k] + su) / (a11[k] + sw);
                    du[k] = (1.0 - VR_OMEGA) * du[k] + VR_OMEGA * nu;
                    let nv = (b2[k] - a12[k] * du[k] + sv) / (a22[k] + sw);
                    dv[k] = (1.0 - VR_OMEGA) * dv[k] + VR_OMEGA * nv;
                }
            }
        }
    }
    for k in 0..w * h {
        u.data[k] += du[k];
        v.data[k] += dv[k];
    }
}

/// Dense flow from `i0` to `i1` (u8-valued planes of equal size): I1(x + flow(x)) ≈ I0(x).
pub fn dis_flow(i0: &Plane, i1: &Plane) -> Flow {
    let (w0, h0) = (i0.w, i0.h);
    let coarsest = ((2.0 * w0 as f64) / (5.0 * PSZ as f64)).log2().floor().max(0.0) as usize;
    let finest = FINEST.min(coarsest);
    let scales: Vec<Scale> = (0..=coarsest)
        .map(|k| {
            let (w, h) = (w0 >> k, h0 >> k);
            let (a, b) = if k == 0 { (i0.clone(), i1.clone()) } else { (round_u8(&resize_area(i0, w, h)), round_u8(&resize_area(i1, w, h))) };
            let (ix, iy) = sobel3(&a);
            let i1e = extend(&b);
            Scale { i0: a, i1: b, i1e, ix, iy }
        })
        .collect();
    let (mut u, mut v) = (Plane::new(scales[coarsest].i0.w, scales[coarsest].i0.h), Plane::new(scales[coarsest].i0.w, scales[coarsest].i0.h));
    for k in (finest..=coarsest).rev() {
        let s = &scales[k];
        if k != coarsest {
            u = resize_linear(&u, s.i0.w, s.i0.h);
            v = resize_linear(&v, s.i0.w, s.i0.h);
            u.data.iter_mut().for_each(|x| *x *= 2.0);
            v.data.iter_mut().for_each(|x| *x *= 2.0);
        }
        let (sx, sy, ws, hs) = inverse_search(s, &u, &v);
        let (du, dv) = densify(s, &sx, &sy, ws, hs);
        u = du;
        v = dv;
        variational_refine(&s.i0, &s.i1, &mut u, &mut v);
    }
    let f = (1 << finest) as f32;
    let (mut u, mut v) = (resize_linear(&u, w0, h0), resize_linear(&v, w0, h0));
    u.data.iter_mut().for_each(|x| *x *= f);
    v.data.iter_mut().for_each(|x| *x *= f);
    Flow { u, v }
}
