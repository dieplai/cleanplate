//! Spatial filters with OpenCV semantics (default border BORDER_REFLECT_101).
use crate::Plane;

/// OpenCV `borderInterpolate(p, n, BORDER_REFLECT_101)`: gfedcb|abcdefgh|gfedcba.
#[inline]
pub(crate) fn reflect101(mut p: isize, n: usize) -> usize {
    let n = n as isize;
    if n == 1 {
        return 0;
    }
    loop {
        if p < 0 {
            p = -p;
        } else if p >= n {
            p = 2 * n - 2 - p;
        } else {
            return p as usize;
        }
    }
}

/// Separable correlation with a horizontal kernel `kx` and a vertical kernel `ky` (anchors at their centres).
fn sep_filter(p: &Plane, kx: &[f32], ky: &[f32]) -> Plane {
    let (w, h) = (p.w, p.h);
    let (rx, ry) = ((kx.len() / 2) as isize, (ky.len() / 2) as isize);
    let mut tmp = Plane::new(w, h);
    for y in 0..h {
        let row = &p.data[y * w..(y + 1) * w];
        for x in 0..w {
            let mut s = 0.0f32;
            for (i, k) in kx.iter().enumerate() {
                s += k * row[reflect101(x as isize + i as isize - rx, w)];
            }
            tmp.data[y * w + x] = s;
        }
    }
    let mut out = Plane::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0f32;
            for (i, k) in ky.iter().enumerate() {
                s += k * tmp.data[reflect101(y as isize + i as isize - ry, h) * w + x];
            }
            out.data[y * w + x] = s;
        }
    }
    out
}

/// OpenCV `getGaussianKernel(n, sigma, CV_32F)` for sigma > 0 (computed in f64, normalised, cast).
fn gaussian_kernel(n: usize, sigma: f64) -> Vec<f32> {
    let c = (n as f64 - 1.0) / 2.0;
    let k: Vec<f64> = (0..n).map(|i| (-((i as f64 - c).powi(2)) / (2.0 * sigma * sigma)).exp()).collect();
    let s: f64 = k.iter().sum();
    k.iter().map(|v| (v / s) as f32).collect()
}

/// `cv2.GaussianBlur(src, (0, 0), sigma)` on a float image: ksize = round(sigma·8 + 1) | 1.
pub fn gaussian_blur(p: &Plane, sigma: f32) -> Plane {
    let n = (((sigma as f64) * 8.0 + 1.0).round() as usize) | 1;
    let k = gaussian_kernel(n, sigma as f64);
    sep_filter(p, &k, &k)
}

/// `cv2.Sobel(src, CV_32F, 1, 0)` and `(0, 1)`, ksize 3, unnormalised.
pub fn sobel3(p: &Plane) -> (Plane, Plane) {
    let d = [-1.0, 0.0, 1.0];
    let s = [1.0, 2.0, 1.0];
    (sep_filter(p, &d, &s), sep_filter(p, &s, &d))
}

/// `cv2.dilate(mask, np.ones((kh, kw)))`: anchor at the centre, pixels outside the image ignored.
pub fn dilate_rect(m: &[bool], w: usize, h: usize, kw: usize, kh: usize) -> Vec<bool> {
    let (rx, ry) = ((kw / 2) as isize, (kh / 2) as isize);
    let mut horiz = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let (lo, hi) = ((x as isize - rx).max(0) as usize, ((x as isize - rx + kw as isize - 1).min(w as isize - 1)) as usize);
            horiz[y * w + x] = m[y * w + lo..=y * w + hi].iter().any(|&v| v);
        }
    }
    let mut out = vec![false; w * h];
    for y in 0..h {
        let (lo, hi) = ((y as isize - ry).max(0) as usize, ((y as isize - ry + kh as isize - 1).min(h as isize - 1)) as usize);
        for x in 0..w {
            out[y * w + x] = (lo..=hi).any(|yy| horiz[yy * w + x]);
        }
    }
    out
}

/// research/pipeline.py `harmonic`: hole initialised to the mean of the known pixels, then Jacobi iterations of the
/// 4-neighbour average with edge (replicate) padding, updating hole pixels only.
pub fn harmonic_fill(p: &Plane, hole: &[bool], iters: usize) -> Plane {
    let (w, h) = (p.w, p.h);
    let known: Vec<f32> = p.data.iter().zip(hole).filter(|(_, &o)| !o).map(|(v, _)| *v).collect();
    let mean = if known.is_empty() { 0.0 } else { (known.iter().map(|&v| v as f64).sum::<f64>() / known.len() as f64) as f32 };
    let mut g = p.clone();
    for (v, &o) in g.data.iter_mut().zip(hole) {
        if o {
            *v = mean;
        }
    }
    let idx: Vec<usize> = (0..w * h).filter(|&i| hole[i]).collect();
    let mut next = g.data.clone();
    for _ in 0..iters {
        for &i in &idx {
            let (x, y) = (i % w, i / w);
            let up = g.data[y.saturating_sub(1) * w + x];
            let down = g.data[(y + 1).min(h - 1) * w + x];
            let left = g.data[y * w + x.saturating_sub(1)];
            let right = g.data[y * w + (x + 1).min(w - 1)];
            next[i] = (up + down + left + right) / 4.0;
        }
        for &i in &idx {
            g.data[i] = next[i];
        }
    }
    g
}
