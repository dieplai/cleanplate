//! The mark catalog: every template PNG under assets/marks/google, embedded, in file-name order (as Python's
//! `sorted(glob)`), with the generator placements research/pipeline.py knows.
use crate::Plane;
use std::sync::OnceLock;

#[derive(Debug)]
pub struct Mark {
    pub name: &'static str,
    /// Trimmed to the bounding box of alpha > 0; values in 0..1.
    pub alpha: Plane,
    /// (right, bottom) margins in pixels at the template's native scale (`KNOWN` in pipeline.py).
    pub known: &'static [(u32, u32)],
    /// Dilation for opaque mode (`OPAQUE` in pipeline.py): the mark is re-drawn per background.
    pub opaque: Option<u32>,
}

macro_rules! mark {
    ($file:literal, $known:expr, $opaque:expr) => {
        (
            $file,
            include_bytes!(concat!("../../../assets/marks/google/", $file)) as &[u8],
            $known,
            $opaque,
        )
    };
}

type Entry = (
    &'static str,
    &'static [u8],
    &'static [(u32, u32)],
    Option<u32>,
);
const FILES: [Entry; 9] = [
    mark!("gemini-diamond-36.png", &[(32, 32)], None),
    mark!("gemini-diamond-48.png", &[(72, 72), (96, 96)], None),
    mark!("gemini-diamond-96.png", &[(64, 64), (192, 192)], None),
    mark!(
        "gemini-diamond-v1-48.png",
        &[(32, 32), (72, 72), (96, 96)],
        None
    ),
    mark!("gemini-diamond-v1-96.png", &[(64, 64), (192, 192)], None),
    mark!("veo-legacy-68.png", &[(17, 18)], None),
    mark!("veo-legacy-99.png", &[], None),
    mark!("veo-small-1080p.png", &[(24, 24)], Some(1)),
    mark!("veo-small-720p.png", &[(16, 16)], None),
];

/// Decode an 8-bit grayscale PNG to alpha in 0..1 (value / 255).
pub fn decode_gray_png(bytes: &[u8]) -> Plane {
    let mut dec = png::Decoder::new(bytes);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().expect("embedded PNG");
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).expect("embedded PNG frame");
    let ch = info.color_type.samples();
    let (w, h) = (info.width as usize, info.height as usize);
    // grayscale templates: take the first channel (all shipped templates are 1-channel)
    Plane {
        w,
        h,
        data: (0..w * h).map(|i| buf[i * ch] as f32 / 255.0).collect(),
    }
}

/// Crop to the bounding box of `alpha > 0` (`ys, xs = np.where(A0 > 0)` in Python).
pub fn trim(a: &Plane) -> Plane {
    let (mut x0, mut y0, mut x1, mut y1) = (a.w, a.h, 0, 0);
    for y in 0..a.h {
        for x in 0..a.w {
            if a.at(x, y) > 0.0 {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
    }
    a.crop(x0, y0, x1 - x0 + 1, y1 - y0 + 1)
}

pub fn catalog() -> &'static [Mark] {
    static C: OnceLock<Vec<Mark>> = OnceLock::new();
    C.get_or_init(|| {
        FILES
            .iter()
            .map(|&(name, bytes, known, opaque)| Mark {
                name,
                alpha: trim(&decode_gray_png(bytes)),
                known,
                opaque,
            })
            .collect()
    })
}

pub fn by_name(name: &str) -> &'static Mark {
    catalog()
        .iter()
        .find(|m| m.name == name)
        .expect("known mark")
}
