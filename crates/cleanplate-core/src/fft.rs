//! 2-D complex FFT on top of rustfft (pure Rust, SIMD, any size; wasm-compatible).
pub use rustfft::num_complex::Complex64 as C;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

/// Smallest 2^a·3^b·5^c ≥ n (sizes rustfft handles fastest).
pub fn nice_size(n: usize) -> usize {
    (n..).find(|&v| {
        let mut x = v;
        for p in [2, 3, 5] {
            while x % p == 0 {
                x /= p;
            }
        }
        x == 1
    })
    .unwrap()
}

/// Forward and inverse plans for an n×m (width n, height m) transform.
pub struct Fft2 {
    pub n: usize,
    pub m: usize,
    row_f: Arc<dyn Fft<f64>>,
    row_i: Arc<dyn Fft<f64>>,
    col_f: Arc<dyn Fft<f64>>,
    col_i: Arc<dyn Fft<f64>>,
}

impl Fft2 {
    pub fn new(n: usize, m: usize) -> Self {
        let mut p = FftPlanner::new();
        Fft2 { n, m, row_f: p.plan_fft_forward(n), row_i: p.plan_fft_inverse(n), col_f: p.plan_fft_forward(m), col_i: p.plan_fft_inverse(m) }
    }

    /// In-place 2-D FFT; rows with index ≥ `rows_used` are known to be zero (skipped). The inverse is unscaled
    /// (rustfft convention); callers divide by n·m.
    pub fn run(&self, a: &mut [C], inverse: bool, rows_used: usize) {
        let (n, m) = (self.n, self.m);
        let (row, col) = if inverse { (&self.row_i, &self.col_i) } else { (&self.row_f, &self.col_f) };
        row.process(&mut a[..rows_used.min(m) * n]);
        let mut t = vec![C::default(); n * m];
        for r in 0..m {
            for c in 0..n {
                t[c * m + r] = a[r * n + c];
            }
        }
        col.process(&mut t);
        for c in 0..n {
            for r in 0..m {
                a[r * n + c] = t[c * m + r];
            }
        }
    }
}
