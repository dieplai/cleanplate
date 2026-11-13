//! Stills: a synthetic textured image with the real V1 diamond at the Gemini still placement (port of
//! tests/test_image.py).
use cleanplate_core::filter::gaussian_blur;
use cleanplate_core::marks::by_name;
use cleanplate_core::still::remove_still;
use cleanplate_core::Plane;

const S: usize = 1024;

fn texture() -> [Plane; 3] {
    let mut seed = 7u64;
    let mut rnd = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 40) as f32 / (1u64 << 24) as f32 * 255.0
    };
    [0, 1, 2].map(|_| {
        let p = Plane { w: S, h: S, data: (0..S * S).map(|_| rnd()).collect() };
        let b = gaussian_blur(&p, 2.0);
        let m = b.data.iter().sum::<f32>() / b.data.len() as f32;
        Plane { w: S, h: S, data: b.data.iter().map(|v| ((v - m) * 3.0 + 120.0).clamp(0.0, 255.0).round()).collect() }
    })
}

fn interleave(ch: &[Plane; 3]) -> Vec<u8> {
    (0..S * S).flat_map(|i| ch.iter().map(move |c| c.data[i] as u8)).collect()
}

#[test]
fn still_synthetic_v1_48() {
    let clean = texture();
    let a = &by_name("gemini-diamond-v1-48.png").alpha;
    let (x0, y0) = (S - 32 - a.w, S - 32 - a.h); // 48 px at margin 32 (2/3 of the size)
    let mut marked = clean.clone();
    for c in marked.iter_mut() {
        for y in 0..a.h {
            for x in 0..a.w {
                let al = a.at(x, y);
                let v = c.at(x0 + x, y0 + y);
                c.set(x0 + x, y0 + y, ((1.0 - al) * v + al * 255.0).round());
            }
        }
    }
    let out = remove_still(&interleave(&marked), S, S, false, -1.0).expect("mark must be found");
    let (mut se, mut n) = (0f64, 0f64);
    for y in y0 - 4..y0 + a.h + 4 {
        for x in x0 - 4..x0 + a.w + 4 {
            for c in 0..3 {
                let d = out.rgb[(y * S + x) * 3 + c] as f64 - clean[c].at(x, y) as f64;
                se += d * d;
                n += 1.0;
            }
        }
    }
    let psnr = 10.0 * (255.0f64 * 255.0 / (se / n)).log10();
    assert!(psnr >= 45.0, "PSNR {psnr:.1}");
}

#[test]
fn still_refuses_clean() {
    assert!(remove_still(&interleave(&texture()), S, S, false, -1.0).is_err());
}
