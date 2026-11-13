//! Weighted-TV MAP refinement: min Σ w (I − μ)² + λ TV_ε(I) over the mask (research/pipeline.py `tv_refine`).
use crate::Plane;

/// Explicit gradient descent with a per-pixel (Jacobi-preconditioned) step 0.9 / (2w + 8λ/ε). Forward differences
/// replicate the last row/column; the divergence wraps around (numpy `np.roll`), exactly as the reference.
pub fn tv_refine(mu: &Plane, w: &Plane, mask: &[bool], lam: &Plane, iters: usize, eps: f32) -> Plane {
    let (nw, nh) = (mu.w, mu.h);
    let mut img = mu.clone();
    let tau: Vec<f32> = w.data.iter().zip(&lam.data).map(|(&wv, &l)| 0.9 / (2.0 * wv + 8.0 * l / eps)).collect();
    let idx: Vec<usize> = (0..nw * nh).filter(|&k| mask[k]).collect();
    let (mut px, mut py) = (vec![0f32; nw * nh], vec![0f32; nw * nh]);
    for _ in 0..iters {
        for y in 0..nh {
            for x in 0..nw {
                let k = y * nw + x;
                let gx = if x + 1 < nw { img.data[k + 1] - img.data[k] } else { 0.0 };
                let gy = if y + 1 < nh { img.data[k + nw] - img.data[k] } else { 0.0 };
                let n = (gx * gx + gy * gy + eps * eps).sqrt();
                px[k] = gx / n;
                py[k] = gy / n;
            }
        }
        let upd: Vec<f32> = idx
            .iter()
            .map(|&k| {
                let (x, y) = (k % nw, k / nw);
                let left = y * nw + (x + nw - 1) % nw;
                let up = ((y + nh - 1) % nh) * nw + x;
                let div = px[k] - px[left] + py[k] - py[up];
                tau[k] * (2.0 * w.data[k] * (img.data[k] - mu.data[k]) - lam.data[k] * div)
            })
            .collect();
        for (&k, u) in idx.iter().zip(upd) {
            img.data[k] -= u;
        }
    }
    img
}
