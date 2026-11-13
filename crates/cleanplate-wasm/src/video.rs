//! Video for the browser: the caller decodes (WebCodecs) and encodes; this keeps the CLI's two-pass shape.
//! pass 1: `push` every I420 frame (only the bottom-right corner + a 64×36 thumbnail are kept);
//! `solve` detects and rebuilds the region; pass 2: `paste` the rebuilt region into each decoded frame.
// ponytail: pass-1 cropping and paste duplicate crates/cleanplate-cli/src/video_io.rs; move both into core when the
// web path stops changing.
use cleanplate_core::detect::{detect_video, sample_step, shot_cuts};
use cleanplate_core::geom::resize_area;
use cleanplate_core::video::{place_alpha, prepare_alpha, rebuild, region_bounds, Params, Rebuilt};
use cleanplate_core::Plane;

pub struct Vid {
    w: usize,
    h: usize,
    cw: usize,
    ch: usize,
    cy: Vec<Vec<u8>>,
    cu: Vec<Vec<u8>>,
    cv: Vec<Vec<u8>>,
    thumbs: Vec<Plane>,
    /// rebuilt region and its top-left in full-frame luma coordinates
    out: Option<(Rebuilt, usize, usize)>,
}

fn crop(pl: &[u8], pw: usize, x0: usize, y0: usize, cw: usize, ch: usize) -> Vec<u8> {
    (0..ch)
        .flat_map(|r| {
            pl[(y0 + r) * pw + x0..(y0 + r) * pw + x0 + cw]
                .iter()
                .copied()
        })
        .collect()
}

fn region(c: &[u8], pw: usize, x0: usize, y0: usize, rw: usize, rh: usize) -> Plane {
    Plane {
        w: rw,
        h: rh,
        data: (0..rh)
            .flat_map(|yy| {
                c[(y0 + yy) * pw + x0..(y0 + yy) * pw + x0 + rw]
                    .iter()
                    .map(|&v| v as f32)
            })
            .collect(),
    }
}

/// feather·O + (1 − feather)·source, rounded like the reference (+0.5 then truncation).
fn paste(plane: &mut [u8], pw: usize, x0: usize, y0: usize, o: &Plane, feather: &Plane) {
    for yy in 0..o.h {
        for xx in 0..o.w {
            let i = (y0 + yy) * pw + x0 + xx;
            let f = feather.at(xx, yy);
            let v = f * o.at(xx, yy) + (1.0 - f) * plane[i] as f32 + 0.5;
            plane[i] = v.clamp(0.0, 255.0) as u8;
        }
    }
}

impl Vid {
    /// Err when the frame size cannot be processed (odd, or too small for the corner search).
    pub fn new(w: usize, h: usize) -> Result<Vid, String> {
        if !w.is_multiple_of(2) || !h.is_multiple_of(2) {
            return Err(format!(
                "odd frame size {w}x{h} is not supported (4:2:0 needs even dimensions)"
            ));
        }
        let (cw, ch) = ((w / 2).min(480) & !1, (h / 2).min(320) & !1);
        Ok(Vid {
            w,
            h,
            cw,
            ch,
            cy: vec![],
            cu: vec![],
            cv: vec![],
            thumbs: vec![],
            out: None,
        })
    }

    fn origin(&self) -> (usize, usize) {
        (self.w - self.cw, self.h - self.ch)
    }

    /// One I420 frame (`w*h*3/2` bytes, planes contiguous).
    pub fn push(&mut self, f: &[u8]) {
        let (w, h, cw, ch) = (self.w, self.h, self.cw, self.ch);
        let (cx0, cy0) = self.origin();
        let (y, rest) = f.split_at(w * h);
        let (u, v) = rest.split_at(w * h / 4);
        self.cy.push(crop(y, w, cx0, cy0, cw, ch));
        self.cu
            .push(crop(u, w / 2, cx0 / 2, cy0 / 2, cw / 2, ch / 2));
        self.cv
            .push(crop(v, w / 2, cx0 / 2, cy0 / 2, cw / 2, ch / 2));
        self.thumbs
            .push(resize_area(&Plane::from_u8(w, h, y), 64, 36));
    }

    /// Detect and rebuild. Returns (0 = ready to paste | 2 = refused, log).
    pub fn solve(&mut self, full_range: bool) -> (u32, String) {
        let n = self.cy.len();
        let (cw, ch) = (self.cw, self.ch);
        let (cx0, cy0) = self.origin();
        if n < 2 {
            return (2, format!("need at least 2 frames, got {n}"));
        }
        let mut log = vec![format!(
            "{}x{}, {n} frames, corner crop {cw}x{ch}",
            self.w, self.h
        )];
        let shots = shot_cuts(&self.thumbs);
        log.push(format!("shots: {} -> {:?}", shots.len(), shots));
        let corners: Vec<Plane> = self
            .cy
            .iter()
            .step_by(sample_step(n))
            .map(|c| Plane::from_u8(cw, ch, c))
            .collect();
        let refs: Vec<&Plane> = corners.iter().collect();
        let det = match detect_video(&refs, self.w, self.h, 0.5) {
            Ok(d) => d,
            Err(r) => {
                log.push(format!("{r} — video left untouched"));
                return (2, log.join("\n"));
            }
        };
        log.push(format!(
            "detected {} x{:.2} at ({},{}) ncc={:.3}{}",
            det.mark.name,
            det.scale,
            det.x + cx0,
            det.y + cy0,
            det.ncc,
            if det.at_known {
                " (known placement)"
            } else {
                ""
            }
        ));
        let prep = prepare_alpha(&det, -1, false);
        if prep.op >= 0 {
            log.push(format!("opaque mode: support dilated {}px", prep.op));
        }
        let r = region_bounds(&prep, cw, ch);
        let ay = place_alpha(&prep, &r);
        let jy: Vec<Plane> = self
            .cy
            .iter()
            .map(|c| region(c, cw, r.x0, r.y0, r.w, r.h))
            .collect();
        let ju: Vec<Plane> = self
            .cu
            .iter()
            .map(|c| region(c, cw / 2, r.x0 / 2, r.y0 / 2, r.w / 2, r.h / 2))
            .collect();
        let jv: Vec<Plane> = self
            .cv
            .iter()
            .map(|c| region(c, cw / 2, r.x0 / 2, r.y0 / 2, r.w / 2, r.h / 2))
            .collect();
        let params = Params {
            tv_range_white: if full_range { 255.0 } else { 235.0 },
            ..Params::default()
        };
        let rb = rebuild(&jy, &ju, &jv, &ay, prep.op, &shots, &params, None);
        for s in &rb.shot_logs {
            log.push(format!(
                "shot {}-{}: motion={:.2}px K={} lambda={:.1} clean-share={:.2}",
                s.s0, s.s1, s.motion, s.k, s.lam, s.clean_share
            ));
        }
        // pass 1 data is no longer needed
        (self.cy, self.cu, self.cv, self.thumbs) = (vec![], vec![], vec![], vec![]);
        self.out = Some((rb, cx0 + r.x0, cy0 + r.y0));
        (0, log.join("\n"))
    }

    /// Paste the rebuilt region into decoded frame `t` (I420, in place). Frames past the rebuilt count are left as is.
    pub fn paste(&self, t: usize, f: &mut [u8]) {
        let Some((rb, x0, y0)) = &self.out else {
            return;
        };
        if t >= rb.y.len() {
            return;
        }
        let w = self.w;
        let (yp, rest) = f.split_at_mut(w * self.h);
        let (up, vp) = rest.split_at_mut(w * self.h / 4);
        paste(yp, w, *x0, *y0, &rb.y[t], &rb.feather_y);
        paste(up, w / 2, x0 / 2, y0 / 2, &rb.u[t], &rb.feather_c);
        paste(vp, w / 2, x0 / 2, y0 / 2, &rb.v[t], &rb.feather_c);
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
