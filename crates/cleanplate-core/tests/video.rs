//! Video region rebuild on synthetic clips with known ground truth.
use cleanplate_core::detect::Detection;
use cleanplate_core::marks::by_name;
use cleanplate_core::video::{place_alpha, prepare_alpha, rebuild, region_bounds, Params};
use cleanplate_core::Plane;

fn det(name: &str, x: usize, y: usize) -> Detection {
    let m = by_name(name);
    Detection {
        mark: m,
        scale: 1.0,
        x,
        y,
        ncc: 0.9,
        at_known: true,
        alpha: m.alpha.clone(),
    }
}

/// Smooth deterministic texture value at (x, y).
fn tex(x: f32, y: f32) -> f32 {
    128.0
        + 50.0 * (0.11 * x).sin() * (0.07 * y).cos()
        + 30.0 * (0.031 * x + 0.05 * y).sin()
        + 20.0 * (0.23 * x - 0.13 * y).cos()
}

struct Clip {
    jy: Vec<Plane>,
    ju: Vec<Plane>,
    jv: Vec<Plane>,
    gt: Vec<Plane>,
    ay: Plane,
    x0: usize,
    y0: usize,
    mbox: (usize, usize, usize, usize),
}

/// A clip of `n` frames whose background moves `speed` px/frame, marked with the 720p Veo glyph at the corner placement.
fn clip(n: usize, speed: f32) -> Clip {
    let d = det("veo-small-720p.png", 443, 295);
    let p = prepare_alpha(&d, -1, false);
    let r = region_bounds(&p, 480, 320);
    let ay = place_alpha(&p, &r);
    let (w, h) = (r.w, r.h);
    let mut seed = 12345u64;
    let mut noise = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as f32 / (1u64 << 31) as f32 - 0.5) * 4.0
    };
    let (mut jy, mut ju, mut jv, mut gt) = (vec![], vec![], vec![], vec![]);
    for t in 0..n {
        let mut g = Plane::new(w, h);
        let mut j = Plane::new(w, h);
        for yy in 0..h {
            for xx in 0..w {
                let v = tex((r.x0 + xx) as f32 - speed * t as f32, (r.y0 + yy) as f32);
                let a = ay.at(xx, yy);
                g.set(xx, yy, v);
                j.set(
                    xx,
                    yy,
                    ((1.0 - a) * v + a * 235.0 + noise())
                        .round()
                        .clamp(0.0, 255.0),
                );
            }
        }
        gt.push(g);
        jy.push(j);
        ju.push(Plane::filled(w / 2, h / 2, 128.0));
        jv.push(Plane::filled(w / 2, h / 2, 128.0));
    }
    let (mx, my) = ((p.mx - r.x0 as i64) as usize, (p.my - r.y0 as i64) as usize);
    Clip {
        jy,
        ju,
        jv,
        gt,
        ay,
        x0: r.x0,
        y0: r.y0,
        mbox: (mx - 2, my - 2, p.alpha.w + 4, p.alpha.h + 4),
    }
}

fn psnr(c: &Clip, out: &[Plane]) -> f64 {
    let (bx, by, bw, bh) = c.mbox;
    let (mut se, mut n) = (0f64, 0f64);
    for (o, g) in out.iter().zip(&c.gt) {
        for y in by..by + bh {
            for x in bx..bx + bw {
                let d = (o.at(x, y).round().clamp(0.0, 255.0) - g.at(x, y)) as f64;
                se += d * d;
                n += 1.0;
            }
        }
    }
    10.0 * (255.0f64 * 255.0 / (se / n)).log10()
}

#[test]
fn region_bounds_portrait_and_1080p() {
    for (name, cw, ch, x, y) in [
        ("gemini-diamond-48.png", 360, 320, 216, 176),
        ("veo-small-1080p.png", 480, 320, 424, 280),
    ] {
        let p = prepare_alpha(&det(name, x, y), -1, false);
        let r = region_bounds(&p, cw, ch);
        assert!(
            r.x0.is_multiple_of(2) && r.y0.is_multiple_of(2) && r.w.is_multiple_of(2) && r.h.is_multiple_of(2),
            "{name}: even-aligned"
        );
        assert!(
            r.x0 + r.w <= cw && r.y0 + r.h <= ch,
            "{name}: inside the crop"
        );
        assert!(
            p.mx as usize >= r.x0 && p.mx as usize + p.alpha.w <= r.x0 + r.w,
            "{name}: mark inside (x)"
        );
        assert!(
            p.my as usize >= r.y0 && p.my as usize + p.alpha.h <= r.y0 + r.h,
            "{name}: mark inside (y)"
        );
    }
}

#[test]
fn short_shot_does_not_panic() {
    let c = clip(10, 0.0);
    let out = rebuild(
        &c.jy,
        &c.ju,
        &c.jv,
        &c.ay,
        -1,
        &[(0, 10)],
        &Params::default(),
        None,
    );
    assert_eq!(out.y.len(), 10);
    let _ = (c.x0, c.y0);
}

#[test]
fn static_synthetic_restores() {
    let c = clip(40, 0.0);
    let out = rebuild(
        &c.jy,
        &c.ju,
        &c.jv,
        &c.ay,
        -1,
        &[(0, 40)],
        &Params::default(),
        None,
    );
    let p = psnr(&c, &out.y);
    assert!(p >= 40.0, "static PSNR {p:.2}");
}

#[test]
fn moving_synthetic_uses_clean_pixels() {
    let c = clip(40, 2.0);
    let out = rebuild(
        &c.jy,
        &c.ju,
        &c.jv,
        &c.ay,
        -1,
        &[(0, 40)],
        &Params::default(),
        None,
    );
    let p = psnr(&c, &out.y);
    assert!(
        out.shot_logs[0].clean_share >= 0.5,
        "clean share {}",
        out.shot_logs[0].clean_share
    );
    assert!(p >= 42.0, "moving PSNR {p:.2}");
}
