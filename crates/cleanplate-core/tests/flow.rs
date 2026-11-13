//! DIS optical flow (OpenCV PRESET_MEDIUM) against OpenCV on real region-sized crops and a synthetic translation.
use cleanplate_core::flow::dis_flow;
use cleanplate_core::Plane;

fn u8_plane(name: &str) -> Plane {
    let p = format!("{}/tests/fixtures/{name}.npy", env!("CARGO_MANIFEST_DIR"));
    let f = npyz::NpyFile::new(std::io::BufReader::new(std::fs::File::open(&p).unwrap())).unwrap();
    let s = f.shape().to_vec();
    Plane::from_u8(s[1] as usize, s[0] as usize, &f.into_vec::<u8>().unwrap())
}
fn cv_flow(name: &str) -> (Vec<f32>, Vec<f32>) {
    let p = format!("{}/tests/fixtures/{name}.npy", env!("CARGO_MANIFEST_DIR"));
    let f = npyz::NpyFile::new(std::io::BufReader::new(std::fs::File::open(&p).unwrap())).unwrap();
    let v = f.into_vec::<f32>().unwrap();
    (
        v.iter().step_by(2).copied().collect(),
        v.iter().skip(1).step_by(2).copied().collect(),
    )
}
fn pct(mut e: Vec<f32>, q: f32) -> f32 {
    e.sort_by(|a, b| a.partial_cmp(b).unwrap());
    e[((e.len() - 1) as f32 * q) as usize]
}
fn interior(w: usize, h: usize, b: usize) -> impl Iterator<Item = usize> {
    (b..h - b).flat_map(move |y| (b..w - b).map(move |x| y * w + x))
}

#[test]
fn dis_recovers_subpixel_translation() {
    let (i0, i1) = (u8_plane("shift_i0"), u8_plane("shift_i1"));
    let f = dis_flow(&i0, &i1);
    let e: Vec<f32> = interior(i0.w, i0.h, 10)
        .map(|i| (f.u.data[i] - 1.3).hypot(f.v.data[i] + 0.7))
        .collect();
    let m = pct(e, 0.5);
    assert!(m < 0.05, "median EPE {m}");
}

#[test]
fn dis_matches_opencv_on_rigid_pairs() {
    // flower market, camera pan, offsets 1 / 4 / 12 frames: same vectors as OpenCV
    for k in 0..3 {
        let (i0, i1) = (
            u8_plane(&format!("flow_i0_{k}")),
            u8_plane(&format!("flow_i1_{k}")),
        );
        let (cu, cv) = cv_flow(&format!("flow_cv_{k}"));
        let f = dis_flow(&i0, &i1);
        let e: Vec<f32> = (0..cu.len())
            .map(|i| (f.u.data[i] - cu[i]).hypot(f.v.data[i] - cv[i]))
            .collect();
        let (m, p90) = (pct(e.clone(), 0.5), pct(e, 0.9));
        assert!(m < 0.15 && p90 < 0.6, "pair {k}: median {m} p90 {p90}");
    }
}

/// Mean |I1(x + f(x)) − I0(x)| over the interior: how well a flow explains the pair.
fn warp_error(i0: &Plane, i1: &Plane, u: &[f32], v: &[f32]) -> f32 {
    use cleanplate_core::geom::{remap, Border, Interp};
    let (w, h) = (i0.w, i0.h);
    let mx = Plane {
        w,
        h,
        data: (0..w * h).map(|k| (k % w) as f32 + u[k]).collect(),
    };
    let my = Plane {
        w,
        h,
        data: (0..w * h).map(|k| (k / w) as f32 + v[k]).collect(),
    };
    let wp = remap(i1, &mx, &my, Interp::Linear, Border::Replicate);
    let idx: Vec<usize> = interior(w, h, 10).collect();
    idx.iter()
        .map(|&k| (wp.data[k] - i0.data[k]).abs())
        .sum::<f32>()
        / idx.len() as f32
}

#[test]
fn dis_explains_real_pairs_as_well_as_opencv() {
    // includes steam moving ~20 px/frame (pairs 3-5), where vectors are ambiguous for any DIS
    for k in 0..6 {
        let (i0, i1) = (
            u8_plane(&format!("flow_i0_{k}")),
            u8_plane(&format!("flow_i1_{k}")),
        );
        let (cu, cv) = cv_flow(&format!("flow_cv_{k}"));
        let f = dis_flow(&i0, &i1);
        let (ours, theirs) = (
            warp_error(&i0, &i1, &f.u.data, &f.v.data),
            warp_error(&i0, &i1, &cu, &cv),
        );
        assert!(
            ours <= 1.1 * theirs + 0.5,
            "pair {k}: warp error {ours} vs OpenCV {theirs}"
        );
    }
}

#[test]
fn dis_zero_on_identical() {
    let i0 = u8_plane("flow_i0_0");
    let f = dis_flow(&i0, &i0);
    let m =
        f.u.data
            .iter()
            .zip(&f.v.data)
            .map(|(u, v)| u.hypot(*v))
            .fold(0.0, f32::max);
    assert!(m < 0.01, "max |flow| {m}");
}
