//! Minimal radix-2 complex FFT (f64), used for template matching on large stills.
#[derive(Clone, Copy, Debug, Default)]
pub struct C {
    pub re: f64,
    pub im: f64,
}

/// In-place iterative FFT of a power-of-two length buffer; `inverse` scales by 1/n.
pub fn fft(a: &mut [C], inverse: bool) {
    let n = a.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = 2.0 * std::f64::consts::PI / len as f64 * if inverse { 1.0 } else { -1.0 };
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (u, v) = (a[start + k], a[start + k + len / 2]);
                let (tr, ti) = (v.re * cr - v.im * ci, v.re * ci + v.im * cr);
                a[start + k] = C { re: u.re + tr, im: u.im + ti };
                a[start + k + len / 2] = C { re: u.re - tr, im: u.im - ti };
                (cr, ci) = (cr * wr - ci * wi, cr * wi + ci * wr);
            }
        }
        len <<= 1;
    }
    if inverse {
        for v in a.iter_mut() {
            v.re /= n as f64;
            v.im /= n as f64;
        }
    }
}

/// 2-D FFT of an n×m (width n, height m) buffer; rows with index ≥ `rows_used` are known to be zero (skipped).
pub fn fft2(a: &mut [C], n: usize, m: usize, inverse: bool, rows_used: usize) {
    for r in 0..rows_used.min(m) {
        fft(&mut a[r * n..(r + 1) * n], inverse);
    }
    let mut col = vec![C::default(); m];
    for c in 0..n {
        for r in 0..m {
            col[r] = a[r * n + c];
        }
        fft(&mut col, inverse);
        for r in 0..m {
            a[r * n + c] = col[r];
        }
    }
}
