//! Pixel geometry with an explicit half-pixel-center convention.
//!
//! Convention (applied uniformly to raw images, crops and rotations):
//! - Pixel with integer index `(c, r)` (column `c`, row `r`, both 0-based) has
//!   its *center* at continuous coordinates `(c + 0.5, r + 0.5)` in its own
//!   local frame.
//! - Sampling at a continuous point uses nearest-pixel center for ROI pixel
//!   access; all analytic sub-pixel work happens later in ESF phase space.
//! - Rotation is by multiples of 90 degrees around the image center, so that
//!   pixel centers map onto pixel centers exactly and the half-pixel convention
//!   is preserved.
//! - Cropping by `(x0, y0)` translates the local frame: a cropped pixel index
//!   `(c, r)` has center `(x0 + c + 0.5, y0 + r + 0.5)` in parent coordinates.

use serde::{Deserialize, Serialize};

/// Raster image: row-major grayscale doubles in `[0, 1]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrayImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<f64>,
}

impl GrayImage {
    pub fn new(width: usize, height: usize, pixels: Vec<f64>) -> Self {
        assert_eq!(pixels.len(), width * height, "pixel buffer size mismatch");
        Self { width, height, pixels }
    }

    pub fn filled(width: usize, height: usize, value: f64) -> Self {
        Self::new(width, height, vec![value; width * height])
    }

    #[inline]
    pub fn at(&self, col: usize, row: usize) -> f64 {
        self.pixels[row * self.width + col]
    }

    #[inline]
    pub fn set(&mut self, col: usize, row: usize, v: f64) {
        self.pixels[row * self.width + col] = v;
    }
}

/// Continuous center coordinate of pixel `(col, row)` in the local frame.
#[inline]
pub fn pixel_center(col: i64, row: i64) -> (f64, f64) {
    (col as f64 + 0.5, row as f64 + 0.5)
}

/// Nearest pixel index whose center is closest to the continuous point.
#[inline]
pub fn center_to_index(x: f64, y: f64) -> (i64, i64) {
    (x.floor() as i64, y.floor() as i64)
}

/// Geometry of a rectangular raster: width, height and origin in parent frame.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Rect {
    pub x0: i64,
    pub y0: i64,
    pub width: usize,
    pub height: usize,
}

impl Rect {
    /// Center coordinates of the four corners, in parent-frame coordinates.
    /// Order: top-left, top-right, bottom-left, bottom-right.
    pub fn corner_centers(&self) -> [(f64, f64); 4] {
        let (x0, y0) = (self.x0 as f64, self.y0 as f64);
        [
            (x0 + 0.5, y0 + 0.5),
            (x0 + self.width as f64 - 0.5, y0 + 0.5),
            (x0 + 0.5, y0 + self.height as f64 - 0.5),
            (x0 + self.width as f64 - 0.5, y0 + self.height as f64 - 0.5),
        ]
    }
}

/// Multiples of 90 degrees, clockwise in the displayed (x=right, y=down) frame.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Rotation {
    R0,
    R90,
    R180,
    R270,
}

impl Rotation {
    pub fn quarter_turns(self) -> i64 {
        match self {
            Rotation::R0 => 0,
            Rotation::R90 => 1,
            Rotation::R180 => 2,
            Rotation::R270 => 3,
        }
    }

    pub fn from_quarter_turns(q: i64) -> Self {
        match q.rem_euclid(4) {
            0 => Rotation::R0,
            1 => Rotation::R90,
            2 => Rotation::R180,
            _ => Rotation::R270,
        }
    }
}

/// A point mapping produced by a rotation/crop transform, in both frames.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct MappedPixel {
    /// Pixel index in the (possibly transformed) local ROI frame.
    pub col: i64,
    pub row: i64,
    /// Its center in the ROI local frame (always `col+0.5, row+0.5`).
    pub center_local: (f64, f64),
    /// The same center mapped into the parent (source image) frame.
    pub center_parent: (f64, f64),
    /// Nearest source pixel index (parent indices).
    pub source_col: i64,
    pub source_row: i64,
}

/// A realized ROI: extracted pixels plus the transform record needed to audit
/// how every local pixel center maps back to the source image.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoiTransform {
    /// Crop rectangle in the *pre-rotation* parent frame, in pixel indices.
    pub crop: Rect,
    pub rotation: Rotation,
    /// Rotated/cropped raster dimensions.
    pub out_width: usize,
    pub out_height: usize,
}

impl RoiTransform {
    pub fn build(crop: Rect, rotation: Rotation) -> Self {
        let swap = matches!(rotation, Rotation::R90 | Rotation::R270);
        let (out_width, out_height) = if swap {
            (crop.height, crop.width)
        } else {
            (crop.width, crop.height)
        };
        Self { crop, rotation, out_width, out_height }
    }

    /// Map a local ROI pixel center back to the source parent frame.
    pub fn map_local_center_to_parent(&self, col: i64, row: i64) -> (f64, f64) {
        let (lx, ly) = pixel_center(col, row);
        let (w, h) = (self.crop.width as f64, self.crop.height as f64);
        let swap = matches!(self.rotation, Rotation::R90 | Rotation::R270);
        // Center of the crop rectangle, about which rotation occurs.
        let cx = self.crop.x0 as f64 + w / 2.0;
        let cy = self.crop.y0 as f64 + h / 2.0;
        // First invert the rotation: local pre-rotation coords inside crop.
        let (px, py) = match self.rotation {
            Rotation::R0 => (lx, ly),
            Rotation::R90 => (w - 1.0 - ly, lx),
            Rotation::R180 => (w - 1.0 - lx, h - 1.0 - ly),
            Rotation::R270 => (ly, h - 1.0 - lx),
        };
        let _ = (w, h, swap);
        (self.crop.x0 as f64 + px, self.crop.y0 as f64 + py)
    }

    pub fn describe_pixel(&self, col: i64, row: i64) -> MappedPixel {
        let center_local = pixel_center(col, row);
        let center_parent = self.map_local_center_to_parent(col, row);
        let (source_col, source_row) = center_to_index(center_parent.0, center_parent.1);
        MappedPixel { col, row, center_local, center_parent, source_col, source_row }
    }
}

/// Extract the ROI raster from a source image using nearest-pixel-center
/// sampling. Returns an error if the crop is outside the source bounds.
pub fn extract_roi(src: &GrayImage, t: &RoiTransform) -> Result<GrayImage, String> {
    let Rect { x0, y0, width, height } = t.crop;
    if x0 < 0
        || y0 < 0
        || x0 as usize + width > src.width
        || y0 as usize + height > src.height
    {
        return Err(format!(
            "crop ({},{}) {}x{} outside source {}x{}",
            x0, y0, width, height, src.width, src.height
        ));
    }
    let mut out = vec![0.0f64; t.out_width * t.out_height];
    for row in 0..t.out_height as i64 {
        for col in 0..t.out_width as i64 {
            let (px, py) = t.map_local_center_to_parent(col, row);
            let (sc, sr) = center_to_index(px, py);
            out[(row as usize) * t.out_width + col as usize] =
                src.at(sc as usize, sr as usize);
        }
    }
    Ok(GrayImage::new(t.out_width, t.out_height, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(w: usize, h: usize) -> GrayImage {
        let mut px = Vec::with_capacity(w * h);
        for r in 0..h {
            for c in 0..w {
                px.push((r * w + c) as f64);
            }
        }
        GrayImage::new(w, h, px)
    }

    #[test]
    fn half_pixel_centers_basic() {
        assert_eq!(pixel_center(0, 0), (0.5, 0.5));
        assert_eq!(pixel_center(3, 7), (3.5, 7.5));
        assert_eq!(center_to_index(3.5, 7.5), (3, 7));
    }

    #[test]
    fn crop_origin_offset_centers() {
        let img = numbered(5, 5);
        let t = RoiTransform::build(
            Rect { x0: 1, y0: 2, width: 3, height: 2 },
            Rotation::R0,
        );
        let roi = extract_roi(&img, &t).unwrap();
        assert_eq!(roi.at(0, 0), img.at(1, 2));
        let m = t.describe_pixel(0, 0);
        assert_eq!(m.center_local, (0.5, 0.5));
        assert_eq!(m.center_parent, (1.5, 2.5));
        assert_eq!((m.source_col, m.source_row), (1, 2));
        assert_eq!(t.describe_pixel(2, 1).center_parent, (3.5, 4.5));
    }

    #[test]
    fn rotation90_preserves_centers_and_values() {
        let img = numbered(4, 3);
        let t = RoiTransform::build(
            Rect { x0: 0, y0: 0, width: 4, height: 3 },
            Rotation::R90,
        );
        assert_eq!((t.out_width, t.out_height), (3, 4));
        let roi = extract_roi(&img, &t).unwrap();
        // Clockwise rotation: local (0,0) comes from source (0, H-1)=(0,2).
        assert_eq!(roi.at(0, 0), img.at(0, 2));
        assert_eq!(roi.at(2, 0), img.at(0, 0));
        assert_eq!(roi.at(2, 3), img.at(3, 0));
        // Center round trip stays on the half-pixel lattice.
        let m = t.describe_pixel(1, 2);
        assert_eq!(m.center_local, (1.5, 2.5));
        assert!(m.center_parent.0.fract() == 0.5 && m.center_parent.1.fract() == 0.5);
        assert_eq!(m.center_parent, (0.5, 1.5));
    }

    #[test]
    fn all_rotations_keep_centers_on_half_pixels() {
        let img = numbered(5, 4);
        for rot in [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270] {
            let t = RoiTransform::build(
                Rect { x0: 1, y0: 0, width: 3, height: 4 },
                rot,
            );
            let roi = extract_roi(&img, &t).unwrap();
            for r in 0..roi.height as i64 {
                for c in 0..roi.width as i64 {
                    let m = t.describe_pixel(c, r);
                    assert!(
                        m.center_parent.0.fract().abs() - 0.5 < 1e-9
                            || (0.5 - m.center_parent.0.fract().abs()).abs() < 1e-9,
                        "rot {:?} center {:?}",
                        rot,
                        m.center_parent
                    );
                    assert_eq!(
                        roi.at(c as usize, r as usize),
                        img.at(m.source_col as usize, m.source_row as usize)
                    );
                }
            }
        }
    }

    #[test]
    fn out_of_bounds_crop_rejected() {
        let img = numbered(3, 3);
        let t = RoiTransform::build(Rect { x0: 2, y0: 2, width: 3, height: 3 }, Rotation::R0);
        assert!(extract_roi(&img, &t).is_err());
    }

    #[test]
    fn quarter_turns_roundtrip() {
        for q in 0..8 {
            let r = Rotation::from_quarter_turns(q);
            assert_eq!(r.quarter_turns(), q.rem_euclid(4));
        }
    }
}
