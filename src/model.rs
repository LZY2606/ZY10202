//! Typed data model: analysis parameters, the full pixel→ESF→LSF→MTF evidence
//! chain, and the parameter fingerprint used to distinguish schemes.

use serde::{Deserialize, Serialize};

use crate::geometry::{Rect, Rotation};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    Auto,
    Vertical,
    Horizontal,
}

/// User-controlled analysis parameters for one ROI / scheme.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalysisParams {
    pub crop: Rect,
    pub rotation: Rotation,
    /// Supersampling factor `k` (ESF bins per pixel).
    pub supersample: u32,
    /// Differentiation window half-width `m` in ESF bins (>=1).
    pub diff_halfwidth: u32,
    pub orientation: Orientation,
    /// ROI-local row indices excluded from both fitting and ESF binning.
    pub excluded_rows: Vec<i64>,
}

impl AnalysisParams {
    /// Stable canonical string used for fingerprinting and replay comparison.
    pub fn canonical(&self) -> String {
        let mut rows = self.excluded_rows.clone();
        rows.sort_unstable();
        let rows_s = rows
            .iter()
            .map(|r| r.to_string())
            .collect::<Vec<_>>()
            .join("_");
        let ori = match self.orientation {
            Orientation::Auto => "auto",
            Orientation::Vertical => "vertical",
            Orientation::Horizontal => "horizontal",
        };
        format!(
            "r{}-c{},{},{},{}-k{}-d{}-o{}-x[{}]",
            self.rotation.quarter_turns(),
            self.crop.x0,
            self.crop.y0,
            self.crop.width,
            self.crop.height,
            self.supersample,
            self.diff_halfwidth,
            ori,
            rows_s
        )
    }
}

/// FNV-1a 64-bit fingerprint (deterministic, no external hashing dependency).
pub fn fingerprint64(s: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x00000100000001b3);
    }
    format!("{hash:016x}")
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FitKind {
    /// x = intercept + slope * y (edge close to vertical)
    Vertical,
    /// y = intercept + slope * x (edge close to horizontal)
    Horizontal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RowObservation {
    pub index: i64,
    /// Sub-pixel crossing coordinate (column for vertical fit, row for horizontal).
    pub crossing: Option<f64>,
    pub detected: bool,
    pub excluded: bool,
    pub predicted: Option<f64>,
    pub residual: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeFit {
    pub kind: FitKind,
    pub intercept: f64,
    pub slope: f64,
    /// Angle of the edge away from the nearest reference axis, in degrees.
    pub angle_deg: f64,
    pub used_samples: usize,
    pub rms_residual: f64,
    pub max_abs_residual: f64,
    pub rows: Vec<RowObservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsfBin {
    pub index: i64,
    /// Signed distance from the edge, in source-pixel units.
    pub distance: f64,
    pub count: u32,
    /// Missing bins are `null`; adjacent bins are never copied into them.
    pub mean: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LsfPoint {
    pub index: i64,
    pub distance: f64,
    /// `null` when any differentiation tap lands on a missing ESF bin.
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtfPoint {
    pub bin: usize,
    pub freq_cyc_per_pixel: f64,
    /// Cycles (line pairs) per mm, only when pixel spacing is known.
    pub freq_lp_mm: Option<f64>,
    pub mtf: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CrossingDirection {
    Descending,
    Ascending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Crossing {
    pub freq_cyc_per_pixel: f64,
    pub freq_lp_mm: Option<f64>,
    pub direction: CrossingDirection,
    /// Index of the MTF point immediately below the crossing frequency.
    pub below_bin: usize,
    /// True when this is the rule-selected MTF50 crossing.
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PitchSegment {
    pub y0: i64,
    pub y1: i64,
    pub pitch_um: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PitchResolution {
    /// Resolved only when every ROI source row falls in segments sharing one
    /// non-null spacing; otherwise cycles/pixel is the only permitted unit.
    pub resolved_pitch_um: Option<f64>,
    pub source_row_min: i64,
    pub source_row_max: i64,
    pub segments_hit: Vec<PitchSegment>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtfSummary {
    pub nyquist_cyc_per_pixel: f64,
    pub mtf_at_nyquist: Option<f64>,
    /// One point below, at, and one above Nyquist (where available).
    pub near_nyquist: Vec<MtfPoint>,
    pub lsf_centroid_distance: Option<f64>,
    pub crossings: Vec<Crossing>,
    pub mtf50_cyc_per_pixel: Option<f64>,
    pub mtf50_lp_mm: Option<f64>,
    /// Human-readable rule statement, identical for every run.
    pub mtf50_rule: String,
    pub nfft: usize,
    pub fft_window_bin_start: i64,
    pub fft_window_bins: usize,
    pub lsf_valid_bin_start: i64,
    pub lsf_valid_bin_end: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PixelAudit {
    pub col: i64,
    pub row: i64,
    pub center_local: (f64, f64),
    pub center_parent: (f64, f64),
    pub source_col: i64,
    pub source_row: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisEvidence {
    pub image_id: String,
    pub label: String,
    pub fingerprint: String,
    pub params: AnalysisParams,
    pub roi_width: usize,
    pub roi_height: usize,
    /// Four ROI corner pixel centers mapped into source coordinates.
    pub corner_audit: Vec<PixelAudit>,
    pub contributed_pixels: usize,
    pub fit: EdgeFit,
    pub esf: Vec<EsfBin>,
    pub lsf: Vec<LsfPoint>,
    pub mtf: Vec<MtfPoint>,
    pub pitch: PitchResolution,
    pub summary: MtfSummary,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredImage {
    pub id: String,
    pub name: String,
    pub width: usize,
    pub height: usize,
    pub grayscale: Vec<f64>,
    pub segments: Vec<PitchSegment>,
    pub bad_rows: Vec<i64>,
}
