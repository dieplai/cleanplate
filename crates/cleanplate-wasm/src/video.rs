//! Video for the browser: the page decodes and encodes (WebCodecs); `cleanplate_core::session` does the rest.
use cleanplate_core::session::Session;
use cleanplate_core::video::Params;

pub struct Vid {
    s: Session,
    w: usize,
    h: usize,
}

impl Vid {
    /// Err when the frame size cannot be processed.
    pub fn new(w: usize, h: usize) -> Result<Vid, String> {
        Ok(Vid {
            s: Session::new(w, h)?,
            w,
            h,
        })
    }

    /// One I420 frame (`w*h*3/2` bytes, planes contiguous).
    pub fn push(&mut self, f: &[u8]) {
        self.s.push(f);
    }

    /// Detect and rebuild. Returns (0 = ready to paste | 2 = refused, log).
    pub fn solve(&mut self, full_range: bool) -> (u32, String) {
        let (cw, ch) = self.s.corner();
        let mut log = vec![format!(
            "{}x{}, {} frames, corner crop {cw}x{ch}",
            self.w,
            self.h,
            self.s.frames()
        )];
        let mut push = |m: String| log.push(m);
        if let Err(r) = self.s.detect(0.5, -1, false, &mut push) {
            push(format!("{r} - video left untouched"));
            return (2, log.join("\n"));
        }
        let params = Params {
            tv_range_white: if full_range { 255.0 } else { 235.0 },
            ..Params::default()
        };
        self.s.rebuild(&params, None, &mut push);
        (0, log.join("\n"))
    }

    /// Paste the rebuilt region into decoded frame `t` (I420, in place).
    pub fn paste(&self, t: usize, f: &mut [u8]) {
        self.s.paste(t, f);
    }
}

#[cfg(test)]
mod tests {
    use super::Vid;

    /// 640x360 I420 frames of a texture panning 2 px/frame; `mark` composites the 720p Veo glyph at its placement.
    fn clip(n: usize, mark: bool) -> Vec<Vec<u8>> {
        let (w, h) = (640, 360);
        let a = &cleanplate_core::marks::by_name("veo-small-720p.png").alpha;
        let (x0, y0) = (w - 16 - a.w, h - 16 - a.h);
        (0..n)
            .map(|t| {
                let mut f = vec![128u8; w * h * 3 / 2];
                for y in 0..h {
                    for x in 0..w {
                        let fx = (x + 2 * t) as f32;
                        let v = 120.0
                            + 50.0 * (0.11 * fx).sin() * (0.07 * y as f32).cos()
                            + 20.0 * (0.031 * fx + 0.05 * y as f32).sin();
                        f[y * w + x] = v as u8;
                    }
                }
                if mark {
                    for y in 0..a.h {
                        for x in 0..a.w {
                            let i = (y0 + y) * w + x0 + x;
                            let al = a.at(x, y);
                            f[i] = ((1.0 - al) * f[i] as f32 + al * 235.0).round() as u8;
                        }
                    }
                }
                f
            })
            .collect()
    }

    #[test]
    fn removes_mark_and_refuses_clean() {
        let (clean, marked) = (clip(24, false), clip(24, true));
        let mut v = Vid::new(640, 360).unwrap();
        marked.iter().for_each(|f| v.push(f));
        let (st, log) = v.solve(false);
        assert_eq!(st, 0, "{log}");
        let err = |a: &[u8]| -> f64 {
            a[..640 * 360]
                .iter()
                .zip(&clean[5][..640 * 360])
                .map(|(&p, &q)| (p as f64 - q as f64).powi(2))
                .sum()
        };
        let mut f = marked[5].clone();
        let before = err(&f);
        v.paste(5, &mut f);
        assert!(err(&f) < before / 10.0, "{} vs {before}\n{log}", err(&f));

        let mut c = Vid::new(640, 360).unwrap();
        clean.iter().for_each(|f| c.push(f));
        assert_eq!(c.solve(false).0, 2);
        assert!(Vid::new(641, 360).is_err());
    }
}
