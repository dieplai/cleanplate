//! cleanplate core: every algorithm on in-memory planes. No file, process or OS access (wasm32-ready).
// pixel loops index several parallel buffers (candidates, weights, planes) by the same index on purpose
#![allow(clippy::needless_range_loop)]
pub mod detect;
pub mod fft;
pub mod filter;
pub mod flow;
pub mod geom;
pub mod marks;
pub mod ncc;
pub mod plane;
pub mod session;
pub mod still;
pub mod tv;
pub mod video;

pub use plane::Plane;

/// Map `0..n` to `f(i)` in order, in parallel when the `parallel` feature is on.
pub(crate) fn par_map<T: Send, F: Fn(usize) -> T + Sync + Send>(n: usize, f: F) -> Vec<T> {
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        (0..n).into_par_iter().map(f).collect()
    }
    #[cfg(not(feature = "parallel"))]
    {
        (0..n).map(f).collect()
    }
}
