//! Video detection and shot cuts against research/pipeline.py on real corner crops (fixtures).
use cleanplate_core::detect::{detect_video, shot_cuts};
use cleanplate_core::Plane;

fn load(name: &str) -> npyz::NpyFile<std::io::BufReader<std::fs::File>> {
    let p = format!("{}/tests/fixtures/{name}.npy", env!("CARGO_MANIFEST_DIR"));
    npyz::NpyFile::new(std::io::BufReader::new(std::fs::File::open(&p).expect(&p))).unwrap()
}
fn corners(tag: &str) -> Vec<Plane> {
    let f = load(&format!("corners_{tag}"));
    let s: Vec<usize> = f.shape().iter().map(|&v| v as usize).collect();
    let px = f.into_vec::<u8>().unwrap();
    px.chunks(s[1] * s[2])
        .map(|c| Plane::from_u8(s[2], s[1], c))
        .collect()
}
fn want(tag: &str) -> Vec<f32> {
    load(&format!("det_{tag}")).into_vec::<f32>().unwrap() // x, y, ncc, scale, ok, known
}

fn check_found(tag: &str, name: &str) {
    let c = corners(tag);
    let refs: Vec<&Plane> = c.iter().collect();
    let w = want(tag);
    let d = detect_video(&refs, 1280, 720, 0.5).expect("mark must be found");
    assert_eq!(d.mark.name, name);
    assert_eq!((d.x, d.y), (w[0] as usize, w[1] as usize));
    assert!(
        (d.ncc - w[2]).abs() <= 0.01,
        "ncc {} vs python {}",
        d.ncc,
        w[2]
    );
    assert!(d.at_known);
}

#[test]
fn detect_finds_veo_small() {
    check_found("veo", "veo-small-720p.png");
}

#[test]
fn detect_finds_diamond() {
    check_found("diamond", "gemini-diamond-48.png");
}

#[test]
fn detect_refuses_clean() {
    let c = corners("clean");
    let refs: Vec<&Plane> = c.iter().collect();
    let r = detect_video(&refs, 1280, 720, 0.5).expect_err("clean clip must be refused");
    assert_eq!(r.floor, 0.75);
    assert!((r.best_ncc - want("clean")[2]).abs() <= 0.01);
}

#[test]
fn shot_cuts_on_concatenation() {
    let thumbs: Vec<Plane> = (0..40)
        .map(|i| Plane::filled(64, 36, if i < 20 { 40.0 } else { 160.0 }))
        .collect();
    assert_eq!(shot_cuts(&thumbs), vec![(0, 20), (20, 40)]);
}
