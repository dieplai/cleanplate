/// A single-channel f32 image, row-major (`data[y * w + x]`).
#[derive(Clone, Debug, PartialEq)]
pub struct Plane {
    pub w: usize,
    pub h: usize,
    pub data: Vec<f32>,
}

impl Plane {
    pub fn new(w: usize, h: usize) -> Self {
        Plane {
            w,
            h,
            data: vec![0.0; w * h],
        }
    }

    pub fn filled(w: usize, h: usize, v: f32) -> Self {
        Plane {
            w,
            h,
            data: vec![v; w * h],
        }
    }

    pub fn from_u8(w: usize, h: usize, px: &[u8]) -> Self {
        assert_eq!(px.len(), w * h);
        Plane {
            w,
            h,
            data: px.iter().map(|&v| v as f32).collect(),
        }
    }

    #[inline]
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.data[y * self.w + x]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, v: f32) {
        self.data[y * self.w + x] = v;
    }

    pub fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> Plane {
        let mut out = Plane::new(w, h);
        for r in 0..h {
            out.data[r * w..(r + 1) * w]
                .copy_from_slice(&self.data[(y + r) * self.w + x..(y + r) * self.w + x + w]);
        }
        out
    }

    /// Pad by `n` pixels on every side with the constant `v` (numpy `np.pad(a, n)` with v = 0).
    pub fn pad_const(&self, n: usize, v: f32) -> Plane {
        let mut out = Plane::filled(self.w + 2 * n, self.h + 2 * n, v);
        for r in 0..self.h {
            let o = (r + n) * out.w + n;
            out.data[o..o + self.w].copy_from_slice(&self.data[r * self.w..(r + 1) * self.w]);
        }
        out
    }

    pub fn max(&self) -> f32 {
        self.data.iter().copied().fold(f32::NEG_INFINITY, f32::max)
    }
}
