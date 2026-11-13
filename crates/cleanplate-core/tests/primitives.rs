//! Golden tests: each primitive against the OpenCV call research/pipeline.py makes (fixtures from
//! research/export_fixtures.py).
use cleanplate_core::filter::{dilate_rect, gaussian_blur, harmonic_fill, sobel3};
use cleanplate_core::geom::{remap, resize_area, resize_cubic, Border, Interp};
use cleanplate_core::Plane;

fn npy(name: &str) -> (Vec<u64>, npyz::NpyFile<std::io::BufReader<std::fs::File>>) {
    let p = format!("{}/tests/fixtures/{name}.npy", env!("CARGO_MANIFEST_DIR"));
    let f = npyz::NpyFile::new(std::io::BufReader::new(std::fs::File::open(&p).expect(&p))).unwrap();
    (f.shape().to_vec(), f)
}
pub fn plane(name: &str) -> Plane {
    let (s, f) = npy(name);
    Plane { w: s[1] as usize, h: s[0] as usize, data: f.into_vec::<f32>().unwrap() }
}
fn mask(name: &str) -> (Vec<bool>, usize, usize) {
    let (s, f) = npy(name);
    (f.into_vec::<u8>().unwrap().into_iter().map(|v| v != 0).collect(), s[1] as usize, s[0] as usize)
}
fn max_err(a: &Plane, b: &Plane) -> f32 {
    assert_eq!((a.w, a.h), (b.w, b.h), "size");
    a.data.iter().zip(&b.data).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

#[test]
fn gaussian_matches_opencv() {
    for (inp, s) in [("a", 1.0), ("a", 3.0), ("b", 1.0), ("b", 3.0)] {
        let e = max_err(&gaussian_blur(&plane(&format!("in_{inp}")), s), &plane(&format!("gauss_{inp}_{s}")));
        assert!(e <= 1e-3, "{inp} sigma {s}: {e}");
    }
}

#[test]
fn sobel_matches_opencv() {
    let (gx, gy) = sobel3(&plane("in_b"));
    assert!(max_err(&gx, &plane("sobel_x_b")) <= 1e-3);
    assert!(max_err(&gy, &plane("sobel_y_b")) <= 1e-3);
}

#[test]
fn dilate_matches_opencv() {
    let (m, w, h) = mask("mask_a");
    for k in [3, 5, 9] {
        let (want, _, _) = mask(&format!("dilate_{k}"));
        assert_eq!(dilate_rect(&m, w, h, k, k), want, "kernel {k}");
    }
}

#[test]
fn harmonic_matches_python() {
    let (hole, _, _) = mask("hole_a");
    assert!(max_err(&harmonic_fill(&plane("in_a"), &hole, 150), &plane("harmonic_a")) <= 1e-3);
}

#[test]
fn resize_area_matches_opencv() {
    assert!(max_err(&resize_area(&plane("in_b"), 32, 24), &plane("area_b_32x24")) <= 1e-3);
    assert!(max_err(&resize_area(&plane("in_t96"), 58, 58), &plane("area_t_58")) <= 1e-3);
}

#[test]
fn resize_cubic_matches_opencv() {
    assert!(max_err(&resize_cubic(&plane("in_a"), 56, 44), &plane("cubic_a_56x44")) <= 1e-2);
}

#[test]
fn remap_matches_opencv() {
    let (src, mx, my) = (plane("in_a"), plane("map_x"), plane("map_y"));
    for (interp, i) in [(Interp::Linear, "linear"), (Interp::Cubic, "cubic")] {
        for (border, b) in [(Border::Replicate, "rep"), (Border::Constant(0.0), "const")] {
            let e = max_err(&remap(&src, &mx, &my, interp, border), &plane(&format!("remap_{i}_{b}")));
            assert!(e <= 1e-2, "{i} {b}: {e}");
        }
    }
}

#[test]
fn match_template_matches_opencv() {
    let r = cleanplate_core::ncc::match_template_ccoeff_normed(&plane("in_b"), &plane("tpl_b"));
    assert!(max_err(&r, &plane("match_b")) <= 1e-4, "{}", max_err(&r, &plane("match_b")));
}

#[test]
fn resize_by_factor_matches_opencv() {
    use cleanplate_core::geom::{resize_area_fx, resize_cubic_fx};
    let v = plane("veo_trim");
    assert!(max_err(&resize_cubic_fx(&v, 1.5), &plane("veo_fx1_5")) <= 1e-4);
    assert!(max_err(&resize_area_fx(&v, 0.667), &plane("veo_fx0_667")) <= 1e-4);
}

#[test]
fn tv_matches_python() {
    let (m, _, _) = mask("tv_mask");
    let out = cleanplate_core::tv::tv_refine(&plane("tv_mu"), &plane("tv_w"), &m, &plane("tv_lam"), 300, 1.0);
    let e = max_err(&out, &plane("tv_out"));
    assert!(e <= 1e-2, "{e}");
}

#[test]
fn match_template_fft_matches_direct() {
    let (img, tpl) = (plane("in_b"), plane("tpl_b"));
    let a = cleanplate_core::ncc::match_template_ccoeff_normed(&img, &tpl);
    let b = cleanplate_core::ncc::match_template_fft(&img, &tpl);
    assert!(max_err(&a, &b) <= 1e-4, "{}", max_err(&a, &b));
}
