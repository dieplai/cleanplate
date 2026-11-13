//! Raw C-ABI exports for the browser (no wasm-bindgen): JS copies RGB into `cp_alloc`ed memory, calls `cp_remove`,
//! reads the result block, then frees both with `cp_free`.
//!
//! Result block (little-endian u32 header): `[status, log_len, rgb_len] ++ log (UTF-8) ++ rgb`.
//! status 0 = cleaned, 2 = refused (no mark found with confidence; `rgb_len` = 0).
use cleanplate_core::still::remove_still;

pub mod video;
use video::Vid;

/// `[status, log_len, rgb_len] ++ log ++ rgb` in a fresh `cp_alloc` block.
fn block(status: u32, log: &str, out: &[u8]) -> *mut u8 {
    let mut v = Vec::with_capacity(12 + log.len() + out.len());
    for x in [status, log.len() as u32, out.len() as u32] {
        v.extend_from_slice(&x.to_le_bytes());
    }
    v.extend_from_slice(log.as_bytes());
    v.extend_from_slice(out);
    let p = cp_alloc(v.len());
    // SAFETY: p was just allocated with room for v.len() bytes
    unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), p, v.len()) };
    p
}

/// Start a video session; null when the size is unsupported (`vid_error` explains).
#[no_mangle]
pub extern "C" fn vid_new(w: usize, h: usize) -> *mut Vid {
    match Vid::new(w, h) {
        Ok(v) => Box::into_raw(Box::new(v)),
        Err(e) => {
            *LAST_ERR.lock().unwrap() = e;
            std::ptr::null_mut()
        }
    }
}

static LAST_ERR: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Why the last `vid_new` returned null, as a result block (status 2).
#[no_mangle]
pub extern "C" fn vid_error() -> *mut u8 {
    block(2, &LAST_ERR.lock().unwrap(), &[])
}

/// Pass 1: one I420 frame of `w*h*3/2` bytes.
///
/// # Safety
/// `v` from `vid_new`; `f` points to `w*h*3/2` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn vid_push(v: *mut Vid, f: *const u8, len: usize) {
    (*v).push(std::slice::from_raw_parts(f, len));
}

/// Detect + rebuild; result block with status 0 (paste next) or 2 (refused) and the log.
///
/// # Safety
/// `v` from `vid_new`.
#[no_mangle]
pub unsafe extern "C" fn vid_solve(v: *mut Vid, full_range: u32) -> *mut u8 {
    let (st, log) = (*v).solve(full_range != 0);
    block(st, &log, &[])
}

/// Pass 2: paste into frame `t` in place.
///
/// # Safety
/// `v` from `vid_new`; `f` points to `w*h*3/2` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn vid_paste(v: *mut Vid, t: usize, f: *mut u8, len: usize) {
    (*v).paste(t, std::slice::from_raw_parts_mut(f, len));
}

/// # Safety
/// `v` from `vid_new`, freed once.
#[no_mangle]
pub unsafe extern "C" fn vid_free(v: *mut Vid) {
    drop(Box::from_raw(v));
}

/// Size the worker pool (multi-threaded build only); returns the thread count in use (1 in the single-threaded build).
#[no_mangle]
pub extern "C" fn cp_threads(n: usize) -> usize {
    #[cfg(feature = "parallel")]
    {
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(n.max(1))
            .build_global();
        rayon::current_num_threads()
    }
    #[cfg(not(feature = "parallel"))]
    {
        let _ = n;
        1
    }
}

/// Allocate `n` bytes of wasm memory for the caller.
#[no_mangle]
pub extern "C" fn cp_alloc(n: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(n);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Free a block from `cp_alloc` or `cp_remove`.
///
/// # Safety
/// `p` must come from `cp_alloc(n)` (or be a `cp_remove` result of total size `n`) and not be freed twice.
#[no_mangle]
pub unsafe extern "C" fn cp_free(p: *mut u8, n: usize) {
    drop(Vec::from_raw_parts(p, 0, n));
}

/// Remove the mark from interleaved RGB (`w*h*3` bytes at `rgb`). `lossy`: the file was JPEG/WebP.
///
/// # Safety
/// `rgb` must point to `w*h*3` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn cp_remove(rgb: *const u8, w: usize, h: usize, lossy: u32) -> *mut u8 {
    let px = std::slice::from_raw_parts(rgb, w * h * 3);
    let (status, log, out) = run(px, w, h, lossy != 0);
    block(status, &log, &out)
}

/// Same contract as the CLI: refuse rather than touch an image without a confident mark.
pub fn run(px: &[u8], w: usize, h: usize, lossy: bool) -> (u32, String, Vec<u8>) {
    match remove_still(px, w, h, lossy, -1.0) {
        Ok(r) => (0, r.log.join("\n"), r.rgb),
        Err(e) => (2, format!("{e} - image left untouched"), vec![]),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn refuses_flat_grey_and_tiny() {
        let (s, log, out) = super::run(&vec![128; 256 * 256 * 3], 256, 256, false);
        assert_eq!((s, out.len()), (2, 0), "{log}");
        assert_eq!(super::run(&[0; 30 * 30 * 3], 30, 30, false).0, 2);
    }

    #[test]
    fn result_block_layout() {
        let px = vec![128u8; 256 * 256 * 3];
        unsafe {
            let inp = super::cp_alloc(px.len());
            std::ptr::copy_nonoverlapping(px.as_ptr(), inp, px.len());
            let r = super::cp_remove(inp, 256, 256, 0);
            let hdr: Vec<u32> = (0..3)
                .map(|i| {
                    u32::from_le_bytes(
                        std::slice::from_raw_parts(r.add(i * 4), 4)
                            .try_into()
                            .unwrap(),
                    )
                })
                .collect();
            assert_eq!(hdr[0], 2);
            let log = std::str::from_utf8(std::slice::from_raw_parts(r.add(12), hdr[1] as usize))
                .unwrap();
            assert!(log.contains("no mark"), "{log}");
            super::cp_free(r, 12 + hdr[1] as usize + hdr[2] as usize);
            super::cp_free(inp, px.len());
        }
    }
}
