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
