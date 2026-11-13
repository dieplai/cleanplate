//! Resampling with OpenCV semantics: resize INTER_AREA / INTER_CUBIC and remap (INTER_LINEAR / INTER_CUBIC).
use crate::Plane;

#[derive(Clone, Copy, Debug)]
pub enum Interp {
    Linear,
    Cubic,
}

#[derive(Clone, Copy, Debug)]
pub enum Border {
    Replicate,
    Constant(f32),
}

/// OpenCV `interpolateCubic` (A = −0.75).
#[inline]
fn cubic_coeffs(x: f32) -> [f32; 4] {
    const A: f32 = -0.75;
    let c0 = ((A * (x + 1.0) - 5.0 * A) * (x + 1.0) + 8.0 * A) * (x + 1.0) - 4.0 * A;
    let c1 = ((A + 2.0) * x - (A + 3.0)) * x * x + 1.0;
    let c2 = ((A + 2.0) * (1.0 - x) - (A + 3.0)) * (1.0 - x) * (1.0 - x) + 1.0;
    [c0, c1, c2, 1.0 - c0 - c1 - c2]
}

/// OpenCV `computeResizeAreaTab`: per destination index, the (source index, weight) pairs.
fn area_tab(ssize: usize, dsize: usize, scale: f64) -> Vec<Vec<(usize, f32)>> {
    (0..dsize)
        .map(|dx| {
            let fsx1 = dx as f64 * scale;
            let fsx2 = fsx1 + scale;
            let cell = scale.min(ssize as f64 - fsx1);
            let (sx1, sx2) = (fsx1.ceil() as usize, (fsx2.floor() as usize).min(ssize));
            let mut t = Vec::new();
            if sx1 as f64 - fsx1 > 1e-3 {
                t.push((sx1 - 1, ((sx1 as f64 - fsx1) / cell) as f32));
            }
            for sx in sx1..sx2 {
                t.push((sx, (1.0 / cell) as f32));
            }
            if fsx2 - (sx2 as f64) > 1e-3 && sx2 < ssize {
                t.push((sx2, ((fsx2 - sx2 as f64).min(1.0).min(cell) / cell) as f32));
            }
            t
        })
        .collect()
}

/// `cv2.resize(src, (w, h), interpolation=cv2.INTER_AREA)` for downscaling.
pub fn resize_area(p: &Plane, w: usize, h: usize) -> Plane {
    area_impl(p, w, h, p.w as f64 / w as f64, p.h as f64 / h as f64)
}

/// `cv2.resize(src, None, fx=f, fy=f, interpolation=cv2.INTER_AREA)`: size rounded, mapping uses exactly 1/f.
pub fn resize_area_fx(p: &Plane, f: f64) -> Plane {
    let (w, h) = fx_size(p, f);
    area_impl(p, w, h, 1.0 / f, 1.0 / f)
}

fn fx_size(p: &Plane, f: f64) -> (usize, usize) {
    ((p.w as f64 * f).round_ties_even() as usize, (p.h as f64 * f).round_ties_even() as usize)
}

fn area_impl(p: &Plane, w: usize, h: usize, sx: f64, sy: f64) -> Plane {
    let (tx, ty) = (area_tab(p.w, w, sx), area_tab(p.h, h, sy));
    let mut tmp = Plane::new(w, p.h);
    for y in 0..p.h {
        for (x, t) in tx.iter().enumerate() {
            tmp.data[y * w + x] = t.iter().map(|&(sx, k)| k * p.data[y * p.w + sx]).sum();
        }
    }
    let mut out = Plane::new(w, h);
    for (y, t) in ty.iter().enumerate() {
        for x in 0..w {
            out.data[y * w + x] = t.iter().map(|&(sy, k)| k * tmp.data[sy * w + x]).sum();
        }
    }
    out
}

/// `cv2.resize(src, (w, h), interpolation=cv2.INTER_CUBIC)`: pixel-centre mapping, clamped taps.
pub fn resize_cubic(p: &Plane, w: usize, h: usize) -> Plane {
    cubic_impl(p, w, h, p.w as f64 / w as f64, p.h as f64 / h as f64)
}

/// `cv2.resize(src, None, fx=f, fy=f, interpolation=cv2.INTER_CUBIC)`.
pub fn resize_cubic_fx(p: &Plane, f: f64) -> Plane {
    let (w, h) = fx_size(p, f);
    cubic_impl(p, w, h, 1.0 / f, 1.0 / f)
}

fn cubic_impl(p: &Plane, w: usize, h: usize, sx: f64, sy: f64) -> Plane {
    let taps = |ssize: usize, dsize: usize, scale: f64| -> Vec<([usize; 4], [f32; 4])> {
        (0..dsize)
            .map(|d| {
                let f = (d as f64 + 0.5) * scale - 0.5;
                let s = f.floor();
                let c = cubic_coeffs((f - s) as f32);
                let s = s as isize;
                let i = [0, 1, 2, 3].map(|k| (s - 1 + k).clamp(0, ssize as isize - 1) as usize);
                (i, c)
            })
            .collect()
    };
    let (tx, ty) = (taps(p.w, w, sx), taps(p.h, h, sy));
    let mut tmp = Plane::new(w, p.h);
    for y in 0..p.h {
        for (x, (i, c)) in tx.iter().enumerate() {
            tmp.data[y * w + x] = (0..4).map(|k| c[k] * p.data[y * p.w + i[k]]).sum();
        }
    }
    let mut out = Plane::new(w, h);
    for (y, (i, c)) in ty.iter().enumerate() {
        for x in 0..w {
            out.data[y * w + x] = (0..4).map(|k| c[k] * tmp.data[i[k] * w + x]).sum();
        }
    }
    out
}

const TAB: f32 = 32.0; // OpenCV INTER_TAB_SIZE: remap quantises coordinates to 1/32 px

/// `cv2.remap(src, mapx, mapy, interp, borderMode=…)`; output has the size of the maps.
pub fn remap(src: &Plane, mapx: &Plane, mapy: &Plane, interp: Interp, border: Border) -> Plane {
    let (w, h) = (src.w as isize, src.h as isize);
    let mut out = Plane::new(mapx.w, mapx.h);
    let fetch = |x: isize, y: isize| -> Option<f32> {
        match border {
            Border::Replicate => Some(src.data[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize]),
            Border::Constant(_) => (x >= 0 && x < w && y >= 0 && y < h).then(|| src.data[(y * w + x) as usize]),
        }
    };
    let cval = match border {
        Border::Constant(v) => v,
        Border::Replicate => 0.0,
    };
    for (o, (&mx, &my)) in out.data.iter_mut().zip(mapx.data.iter().zip(&mapy.data)) {
        let (ix, iy) = ((mx * TAB).round() as isize, (my * TAB).round() as isize);
        let (sx, sy) = (ix.div_euclid(TAB as isize), iy.div_euclid(TAB as isize));
        let (fx, fy) = (ix.rem_euclid(TAB as isize) as f32 / TAB, iy.rem_euclid(TAB as isize) as f32 / TAB);
        *o = match interp {
            Interp::Linear => {
                if matches!(border, Border::Constant(_)) && (sx >= w || sx + 1 < 0 || sy >= h || sy + 1 < 0) {
                    cval
                } else {
                    let v = |x, y| fetch(x, y).unwrap_or(cval);
                    let (wx, wy) = ([1.0 - fx, fx], [1.0 - fy, fy]);
                    wy[0] * (wx[0] * v(sx, sy) + wx[1] * v(sx + 1, sy)) + wy[1] * (wx[0] * v(sx, sy + 1) + wx[1] * v(sx + 1, sy + 1))
                }
            }
            Interp::Cubic => {
                let (x0, y0) = (sx - 1, sy - 1);
                if matches!(border, Border::Constant(_)) && (x0 >= w || x0 + 4 <= 0 || y0 >= h || y0 + 4 <= 0) {
                    cval
                } else {
                    let (cx, cy) = (cubic_coeffs(fx), cubic_coeffs(fy));
                    let mut s = 0.0;
                    for j in 0..4 {
                        let mut r = 0.0;
                        for i in 0..4 {
                            r += cx[i] * fetch(x0 + i as isize, y0 + j as isize).unwrap_or(cval);
                        }
                        s += cy[j] * r;
                    }
                    s
                }
            }
        };
    }
    out
}
