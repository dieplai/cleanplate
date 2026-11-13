//! `cv2.matchTemplate(img, tpl, cv2.TM_CCOEFF_NORMED)` and `cv2.minMaxLoc` (max part).
use crate::Plane;

/// Zero-mean normalised cross-correlation for every placement of `tpl` inside `img`
/// (output size (W − w + 1) × (H − h + 1)), including OpenCV's handling of near-flat windows.
pub fn match_template_ccoeff_normed(img: &Plane, tpl: &Plane) -> Plane {
    let (iw, ih, tw, th) = (img.w, img.h, tpl.w, tpl.h);
    let (ow, oh) = (iw - tw + 1, ih - th + 1);
    let n = (tw * th) as f64;
    let tmean = tpl.data.iter().map(|&v| v as f64).sum::<f64>() / n;
    let tz: Vec<f64> = tpl.data.iter().map(|&v| v as f64 - tmean).collect();
    let tnorm = tz.iter().map(|v| v * v).sum::<f64>().sqrt();
    // integral images of I and I² (f64) for the window statistics
    let (sw, mut s1, mut s2) = (iw + 1, vec![0f64; (iw + 1) * (ih + 1)], vec![0f64; (iw + 1) * (ih + 1)]);
    for y in 0..ih {
        for x in 0..iw {
            let v = img.data[y * iw + x] as f64;
            let i = (y + 1) * sw + x + 1;
            s1[i] = v + s1[i - 1] + s1[i - sw] - s1[i - sw - 1];
            s2[i] = v * v + s2[i - 1] + s2[i - sw] - s2[i - sw - 1];
        }
    }
    let rect = |s: &[f64], x: usize, y: usize| s[(y + th) * sw + x + tw] - s[y * sw + x + tw] - s[(y + th) * sw + x] + s[y * sw + x];
    let row = |y: usize| -> Vec<f32> {
        (0..ow)
            .map(|x| {
                let mut num = 0f64;
                for r in 0..th {
                    let ir = &img.data[(y + r) * iw + x..(y + r) * iw + x + tw];
                    let tr = &tz[r * tw..(r + 1) * tw];
                    num += ir.iter().zip(tr).map(|(&a, &b)| a as f64 * b).sum::<f64>();
                }
                let (sum, sum2) = (rect(&s1, x, y), rect(&s2, x, y));
                let t = (sum2 - sum * sum / n).max(0.0).sqrt() * tnorm;
                (if num.abs() < t {
                    num / t
                } else if num.abs() < t * 1.125 {
                    num.signum()
                } else {
                    0.0
                }) as f32
            })
            .collect()
    };
    let rows: Vec<Vec<f32>> = crate::par_map(oh, row);
    Plane { w: ow, h: oh, data: rows.concat() }
}

/// First (row-major) maximum: (value, x, y).
pub fn max_loc(p: &Plane) -> (f32, usize, usize) {
    let mut best = (f32::NEG_INFINITY, 0, 0);
    for (i, &v) in p.data.iter().enumerate() {
        if v > best.0 {
            best = (v, i % p.w, i / p.w);
        }
    }
    best
}

/// Precomputed image side of an FFT-based TM_CCOEFF_NORMED: the image spectrum and its integral images.
pub struct CorrPlan {
    img_w: usize,
    img_h: usize,
    n: usize,
    m: usize,
    fft: crate::fft::Fft2,
    spec: Vec<crate::fft::C>,
    s1: Vec<f64>,
    s2: Vec<f64>,
}

impl CorrPlan {
    pub fn new(img: &Plane) -> Self {
        use crate::fft::{nice_size, Fft2, C};
        let (n, m) = (nice_size(img.w), nice_size(img.h));
        let fft = Fft2::new(n, m);
        let mut spec = vec![C::default(); n * m];
        for y in 0..img.h {
            for x in 0..img.w {
                spec[y * n + x].re = img.at(x, y) as f64;
            }
        }
        fft.run(&mut spec, false, img.h);
        let sw = img.w + 1;
        let (mut s1, mut s2) = (vec![0f64; sw * (img.h + 1)], vec![0f64; sw * (img.h + 1)]);
        for y in 0..img.h {
            for x in 0..img.w {
                let v = img.at(x, y) as f64;
                let i = (y + 1) * sw + x + 1;
                s1[i] = v + s1[i - 1] + s1[i - sw] - s1[i - sw - 1];
                s2[i] = v * v + s2[i - 1] + s2[i - sw] - s2[i - sw - 1];
            }
        }
        CorrPlan { img_w: img.w, img_h: img.h, n, m, fft, spec, s1, s2 }
    }

    /// Same result as [`match_template_ccoeff_normed`] (correlation through the image spectrum).
    pub fn match_template(&self, tpl: &Plane) -> Plane {
        use crate::fft::C;
        let (n, m) = (self.n, self.m);
        let (mut ts, tnorm) = self.zero_mean(tpl, |v| C { re: v, im: 0.0 });
        self.fft.run(&mut ts, false, tpl.h);
        for (t, s) in ts.iter_mut().zip(&self.spec) {
            *t = C { re: s.re * t.re + s.im * t.im, im: s.im * t.re - s.re * t.im }; // spec · conj(tspec)
        }
        self.fft.run(&mut ts, true, m);
        let sc = 1.0 / (n * m) as f64;
        self.normalise(tpl.w, tpl.h, tnorm, |i| ts[i].re * sc)
    }

    /// Two templates with one forward and one inverse FFT: t1 in the real part, t2 in the imaginary part.
    pub fn match_pair(&self, t1: &Plane, t2: &Plane) -> (Plane, Plane) {
        use crate::fft::C;
        let (n, m) = (self.n, self.m);
        let (mut z, n1) = self.zero_mean(t1, |v| C { re: v, im: 0.0 });
        let n2 = self.add_imag(&mut z, t2);
        self.fft.run(&mut z, false, t1.h.max(t2.h));
        let mut y = vec![C::default(); n * m];
        for ky in 0..m {
            for kx in 0..n {
                let (k, km) = (ky * n + kx, ((m - ky) % m) * n + (n - kx) % n);
                let (zk, zc) = (z[k], C { re: z[km].re, im: -z[km].im }); // Z[k], conj(Z[-k])
                // T1 = (Z[k] + conj Z[-k]) / 2, T2 = (Z[k] - conj Z[-k]) / 2i
                let t1 = C { re: 0.5 * (zk.re + zc.re), im: 0.5 * (zk.im + zc.im) };
                let t2 = C { re: 0.5 * (zk.im - zc.im), im: -0.5 * (zk.re - zc.re) };
                // Y = S · (conj T1 + i · conj T2)
                let f = C { re: t1.re + t2.im, im: -t1.im + t2.re };
                let s = self.spec[k];
                y[k] = C { re: s.re * f.re - s.im * f.im, im: s.re * f.im + s.im * f.re };
            }
        }
        self.fft.run(&mut y, true, m);
        let sc = 1.0 / (n * m) as f64;
        (self.normalise(t1.w, t1.h, n1, |i| y[i].re * sc), self.normalise(t2.w, t2.h, n2, |i| y[i].im * sc))
    }

    /// Write the zero-mean `tpl` into the imaginary part of `z`; returns its norm.
    fn add_imag(&self, z: &mut [crate::fft::C], tpl: &Plane) -> f64 {
        let cnt = (tpl.w * tpl.h) as f64;
        let tmean = tpl.data.iter().map(|&v| v as f64).sum::<f64>() / cnt;
        let mut tnorm = 0f64;
        for y in 0..tpl.h {
            for x in 0..tpl.w {
                let v = tpl.at(x, y) as f64 - tmean;
                z[y * self.n + x].im = v;
                tnorm += v * v;
            }
        }
        tnorm.sqrt()
    }

    fn zero_mean(&self, tpl: &Plane, put: impl Fn(f64) -> crate::fft::C) -> (Vec<crate::fft::C>, f64) {
        let cnt = (tpl.w * tpl.h) as f64;
        let tmean = tpl.data.iter().map(|&v| v as f64).sum::<f64>() / cnt;
        let mut ts = vec![crate::fft::C::default(); self.n * self.m];
        let mut tnorm = 0f64;
        for y in 0..tpl.h {
            for x in 0..tpl.w {
                let v = tpl.at(x, y) as f64 - tmean;
                ts[y * self.n + x] = put(v);
                tnorm += v * v;
            }
        }
        (ts, tnorm.sqrt())
    }

    fn normalise(&self, tw: usize, th: usize, tnorm: f64, num_at: impl Fn(usize) -> f64) -> Plane {
        let cnt = (tw * th) as f64;
        let (ow, oh, sw) = (self.img_w - tw + 1, self.img_h - th + 1, self.img_w + 1);
        let rect = |s: &[f64], x: usize, y: usize| s[(y + th) * sw + x + tw] - s[y * sw + x + tw] - s[(y + th) * sw + x] + s[y * sw + x];
        let mut out = Plane::new(ow, oh);
        for y in 0..oh {
            for x in 0..ow {
                let num = num_at(y * self.n + x);
                let (sum, sum2) = (rect(&self.s1, x, y), rect(&self.s2, x, y));
                let t = (sum2 - sum * sum / cnt).max(0.0).sqrt() * tnorm;
                out.data[y * ow + x] = (if num.abs() < t {
                    num / t
                } else if num.abs() < t * 1.125 {
                    num.signum()
                } else {
                    0.0
                }) as f32;
            }
        }
        out
    }
}

/// FFT-based [`match_template_ccoeff_normed`] for large templates/images.
pub fn match_template_fft(img: &Plane, tpl: &Plane) -> Plane {
    CorrPlan::new(img).match_template(tpl)
}
