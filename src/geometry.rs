//! ROI geometry under the half-pixel convention.
//!
//! Pixel `(x, y)` (zero-based indices) has its center at `(x + 0.5, y + 0.5)`
//! in the *image* coordinate frame, x right and y down, units = pixels.
//!
//! An ROI is the half-open rectangle `[x0, x0+w) x [y0, y0+h)` of original
//! pixels. Rotating the ROI by a multiple of 90 degrees only PERMUTES pixel
//! indices — no resampling, no fractional coordinates — therefore every local
//! pixel center maps exactly onto an original pixel center. Local axes:
//! `u` scans across the edge, `v` runs along it.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct Rotation(pub u16);

impl Rotation {
    pub fn clockwise_turns(self) -> u8 {
        ((self.0 % 360) / 90) as u8
    }
    pub fn is_swapped(self) -> bool {
        matches!(self.clockwise_turns(), 1 | 3)
    }
}

impl TryFrom<u16> for Rotation {
    type Error = String;
    fn try_from(v: u16) -> Result<Self, Self::Error> {
        if v % 90 != 0 || v > 270 {
            Err(format!("rotation must be 0/90/180/270, got {v}"))
        } else {
            Ok(Rotation(v))
        }
    }
}

impl From<Rotation> for u16 {
    fn from(r: Rotation) -> u16 {
        r.0
    }
}

impl Default for Rotation {
    fn default() -> Self {
        Rotation(0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Roi {
    pub x0: u32,
    pub y0: u32,
    pub w: u32,
    pub h: u32,
    #[serde(default)]
    pub rotation: Rotation,
}

impl Roi {
    /// Local image dimensions (u-count, v-count) after rotation.
    pub fn local_size(&self) -> (u32, u32) {
        if self.rotation.is_swapped() {
            (self.h, self.w)
        } else {
            (self.w, self.h)
        }
    }

    /// Map a local pixel index to the original image pixel index.
    /// `u in 0..local_w`, `v in 0..local_h`.
    pub fn map_pixel(&self, u: u32, v: u32) -> (u32, u32) {
        let (lw, lh) = self.local_size();
        assert!(u < lw && v < lh, "local pixel ({u},{v}) outside {lw}x{lh}");
        let (x0, y0, w, h) = (self.x0, self.y0, self.w, self.h);
        match self.rotation.clockwise_turns() {
            0 => (x0 + u, y0 + v),
            1 => (x0 + w - 1 - v, y0 + u),
            2 => (x0 + w - 1 - u, y0 + h - 1 - v),
            _ => (x0 + v, y0 + h - 1 - u),
        }
    }

    /// Map a local pixel CENTER to the original image center frame.
    /// Returned coordinates are always of the form integer + 0.5.
    pub fn map_center(&self, u: u32, v: u32) -> (f64, f64) {
        let (x, y) = self.map_pixel(u, v);
        (x as f64 + 0.5, y as f64 + 0.5)
    }

    /// All original pixels touched by a local row (for evidence inspection).
    #[allow(dead_code)]
    pub fn local_row_original_trace(&self, v: u32) -> Vec<(u32, u32)> {
        let (lw, _) = self.local_size();
        (0..lw).map(|u| self.map_pixel(u, v)).collect()
    }

    pub fn validate(&self, img_w: u32, img_h: u32) -> Result<(), String> {
        if self.w == 0 || self.h == 0 {
            return Err("ROI must have non-zero size".into());
        }
        let x1 = self.x0.checked_add(self.w).ok_or("x0+w overflow")?;
        let y1 = self.y0.checked_add(self.h).ok_or("y0+h overflow")?;
        if x1 > img_w || y1 > img_h {
            return Err(format!("ROI ({x1}x{y1}) exceeds image {img_w}x{img_h}"));
        }
        Ok(())
    }
}
