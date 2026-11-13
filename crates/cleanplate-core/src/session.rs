//! One video cleaned in two passes over its frames, whatever decodes them (ffmpeg pipes in the CLI, WebCodecs in the
//! browser):
//! 1. [`Session::push`] every I420 frame: only the bottom-right corner (Y, U, V) and a 64×36 thumbnail are kept;
//! 2. [`Session::detect`] the mark and the shots, then [`Session::rebuild`] the mark region of every frame;
//! 3. [`Session::paste`] the rebuilt region into each frame of a second decode, then encode.
use crate::detect::{detect_video, sample_step, shot_cuts, Refusal, RefusalKind};
use crate::geom::resize_area;
use crate::video::{place_alpha, prepare_alpha, rebuild, region_bounds, Guide, Params, Rebuilt};
use crate::Plane;

/// The rebuilt region in full-frame luma pixels (a guide clip is resampled onto it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

/// What [`Session::detect`] found.
#[derive(Clone, Copy, Debug)]
pub struct Found {
    pub rect: Rect,
    /// The mark is treated as opaque (AI-upscaled downloads): a guide clip can fill it.
    pub opaque: bool,
}

/// Detection result kept between `detect` and `rebuild`.
struct Pending {
    shots: Vec<(usize, usize)>,
    op: i32,
    alpha: Plane,
    jy: Vec<Plane>,
    ju: Vec<Plane>,
    jv: Vec<Plane>,
    rect: Rect,
}

pub struct Session {
    w: usize,
    h: usize,
    cw: usize,
    ch: usize,
    cy: Vec<Vec<u8>>,
    cu: Vec<Vec<u8>>,
    cv: Vec<Vec<u8>>,
    thumbs: Vec<Plane>,
    pending: Option<Pending>,
    out: Option<(Rebuilt, Rect)>,
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

impl Session {
    /// Err when the frame size cannot be processed (4:2:0 needs even dimensions).
    pub fn new(w: usize, h: usize) -> Result<Session, String> {
        if !w.is_multiple_of(2) || !h.is_multiple_of(2) {
            return Err(format!(
                "odd frame size {w}x{h} is not supported (4:2:0 needs even dimensions)"
            ));
        }
        let (cw, ch) = ((w / 2).min(480) & !1, (h / 2).min(320) & !1);
        Ok(Session {
            w,
            h,
            cw,
            ch,
            cy: vec![],
            cu: vec![],
            cv: vec![],
            thumbs: vec![],
            pending: None,
            out: None,
        })
    }

    /// Frames pushed so far.
    pub fn frames(&self) -> usize {
        self.cy.len()
    }

    /// Size of the kept corner (luma).
    pub fn corner(&self) -> (usize, usize) {
        (self.cw, self.ch)
    }

    fn origin(&self) -> (usize, usize) {
        (self.w - self.cw, self.h - self.ch)
    }

    /// Pass 1: one I420 frame (`w·h·3/2` bytes, planes contiguous).
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

    /// Shots and the mark. `opaque`: −1 automatic, else the support dilation in px; `has_guide` enables guide mode.
    pub fn detect(
        &mut self,
        min_ncc: f32,
        opaque: i32,
        has_guide: bool,
        log: &mut dyn FnMut(String),
    ) -> Result<Found, Refusal> {
        let n = self.frames();
        if n == 0 {
            return Err(Refusal {
                kind: RefusalKind::NoFrames,
                value: 0.0,
                floor: 1.0,
            });
        }
        let (cw, ch) = (self.cw, self.ch);
        let (cx0, cy0) = self.origin();
        let shots = shot_cuts(&self.thumbs);
        log(format!("shots: {} -> {:?}", shots.len(), shots));
        // only the frames detection samples become f32 (≈120), not the whole clip
        let corners: Vec<Plane> = self
            .cy
            .iter()
            .step_by(sample_step(n))
            .map(|c| Plane::from_u8(cw, ch, c))
            .collect();
        let refs: Vec<&Plane> = corners.iter().collect();
        let det = detect_video(&refs, self.w, self.h, min_ncc)?;
        log(format!(
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
        let prep = prepare_alpha(&det, opaque, has_guide);
        if prep.op >= 0 {
            log(format!("opaque mode: support dilated {}px", prep.op));
        }
        let r = region_bounds(&prep, cw, ch);
        let alpha = place_alpha(&prep, &r);
        let jy = self
            .cy
            .iter()
            .map(|c| region(c, cw, r.x0, r.y0, r.w, r.h))
            .collect();
        let ju = self
            .cu
            .iter()
            .map(|c| region(c, cw / 2, r.x0 / 2, r.y0 / 2, r.w / 2, r.h / 2))
            .collect();
        let jv = self
            .cv
            .iter()
            .map(|c| region(c, cw / 2, r.x0 / 2, r.y0 / 2, r.w / 2, r.h / 2))
            .collect();
        // the pass-1 corners are no longer needed: only the region is
        (self.cy, self.cu, self.cv, self.thumbs) = (vec![], vec![], vec![], vec![]);
        let rect = Rect {
            x: cx0 + r.x0,
            y: cy0 + r.y0,
            w: r.w,
            h: r.h,
        };
        self.pending = Some(Pending {
            shots,
            op: prep.op,
            alpha,
            jy,
            ju,
            jv,
            rect,
        });
        Ok(Found {
            rect,
            opaque: prep.op >= 0,
        })
    }

    /// Rebuild the mark region of every frame (after a successful `detect`; a no-op otherwise).
    pub fn rebuild(&mut self, params: &Params, guide: Option<&Guide>, log: &mut dyn FnMut(String)) {
        let Some(p) = self.pending.take() else { return };
        let guide = guide.filter(|_| p.op >= 0);
        let rb = rebuild(&p.jy, &p.ju, &p.jv, &p.alpha, p.op, &p.shots, params, guide);
        for s in &rb.shot_logs {
            log(format!(
                "shot {}-{}: motion={:.2}px K={} lambda={:.1} clean-share={:.2}",
                s.s0, s.s1, s.motion, s.k, s.lam, s.clean_share
            ));
        }
        self.out = Some((rb, p.rect));
    }

    /// Pass 2: paste the rebuilt region into decoded frame `t` (I420, in place). Frames beyond the rebuilt ones, or
    /// any frame before `rebuild`, are left as they are.
    pub fn paste(&self, t: usize, f: &mut [u8]) {
        let Some((rb, r)) = &self.out else { return };
        if t >= rb.y.len() {
            return;
        }
        let w = self.w;
        let (yp, rest) = f.split_at_mut(w * self.h);
        let (up, vp) = rest.split_at_mut(w * self.h / 4);
        paste(yp, w, r.x, r.y, &rb.y[t], &rb.feather_y);
        paste(up, w / 2, r.x / 2, r.y / 2, &rb.u[t], &rb.feather_c);
        paste(vp, w / 2, r.x / 2, r.y / 2, &rb.v[t], &rb.feather_c);
    }
}
