//! Slanted-edge analysis pipeline with the full evidence chain kept:
//! pixels -> per-row edge detections -> line fit residuals -> supersampled
//! ESF bins (empty bins stay null) -> LSF (pluggable derivative kernel) ->
//! windowed DFT -> normalized MTF -> MTF50 with an explicit crossing rule.
//!
//! Frequency conventions:
//!   * internal unit: cycles per pixel (Nyquist = 0.5)
//!   * lp/mm with pitch p[um]: f_lpmm = f_cpp * 1000 / p
//! When pitch is unknown the run only reports cycles per pixel.

use serde::{Deserialize, Serialize};

use crate::geometry::Roi;
use crate::image::GrayImage;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivativeKernel {
    Central3,
    FivePoint,
    SevenPoint,
}

impl DerivativeKernel {
    pub fn half_width(self) -> usize {
        match self {
            DerivativeKernel::Central3 => 1,
            DerivativeKernel::FivePoint => 2,
            DerivativeKernel::SevenPoint => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpectralWindow {
    Hann,
    Rectangular,
}

impl Default for SpectralWindow {
    fn default() -> Self {
        SpectralWindow::Hann
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PitchMode {
    #[serde(alias = "auto")]
    Auto,
    Manual,
    Ignore,
}

impl Default for PitchMode {
    fn default() -> Self {
        PitchMode::Auto
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnalysisParams {
    pub roi: Roi,
    /// local rows excluded from the fit and projection
    #[serde(default)]
    pub excluded_rows: Vec<u32>,
    #[serde(default = "default_supersample")]
    pub supersample: u32,
    pub derivative: DerivativeKernel,
    #[serde(default)]
    pub pitch_mode: PitchMode,
    /// required (in micrometres) when pitch_mode = manual
    #[serde(default)]
    pub manual_pitch_um: Option<f64>,
    /// half-width, in supersampled bins, of the spectral window core
    #[serde(default = "default_window_half_bins")]
    pub window_half_bins: u32,
    #[serde(default)]
    pub spectral_window: SpectralWindow,
}

fn default_supersample() -> u32 {
    4
}
fn default_window_half_bins() -> u32 {
    64
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RowEdge {
    pub row: u32,
    pub detected_edge_u: Option<f64>,
    pub low: f64,
    pub high: f64,
    pub contrast: f64,
    pub included: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FitResidual {
    pub row: u32,
    pub residual_px: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeFit {
    /// edge_u(v) = intercept + slope * v, local pixel-center units
    pub slope: f64,
    pub intercept: f64,
    pub angle_deg: f64,
    pub rmse_px: f64,
    pub max_abs_residual_px: f64,
    pub n_rows: usize,
    pub residuals: Vec<FitResidual>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EsfBin {
    pub index: i64,
    pub center_px: f64,
    pub count: u32,
    /// mean code value in 0..=1; null when no pixel landed in the bin
    pub value: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LsfPoint {
    pub index: i64,
    pub center_px: f64,
    pub value: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MtfPoint {
    pub f_cpp: f64,
    pub f_lpmm: Option<f64>,
    pub mtf: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Crossing {
    /// interpolation of the crossing location on the c/p grid
    pub f_cpp: f64,
    pub f_lpmm: Option<f64>,
    /// zero-based index of the *downward* (or upward) crossing segment
    pub segment_index: usize,
    pub direction: String,
    /// which lobe this crossing belongs to; lobe 0 is the main lobe
    pub lobe: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NyquistBehavior {
    pub mtf_at_nyquist: f64,
    pub mtf_at_half_nyquist: f64,
    pub mean_04_05: f64,
    pub mean_05_06: f64,
    pub rises_after_nyquist: bool,
    pub alias_ratio: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CentroidInfo {
    /// signed first moment of the trimmed LSF, in pixels (0 = fitted edge)
    pub centroid_px: f64,
    pub peak_index: i64,
    pub peak_px: f64,
    pub fwhm_px: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PitchResolution {
    pub mode: String,
    pub pitch_um: Option<f64>,
    pub resolved: bool,
    pub note: String,
    /// which bands the included ROI rows intersect (for the evidence page)
    pub bands: Vec<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PixelSample {
    pub u: u32,
    pub v: u32,
    pub ox: u32,
    pub oy: u32,
    pub center_x: f64,
    pub center_y: f64,
    pub value01: f64,
    pub signed_distance_px: f64,
    pub bin_index: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub params_fingerprint: String,
    pub params_json: String,
    pub rows: Vec<RowEdge>,
    pub fit: EdgeFit,
    pub pitch: PitchResolution,
    pub bins: Vec<EsfBin>,
    pub lsf: Vec<LsfPoint>,
    pub mtf: Vec<MtfPoint>,
    pub crossings: Vec<Crossing>,
    pub mtf50: Option<Crossing>,
    pub crossing_rule: String,
    pub nyquist: NyquistBehavior,
    pub centroid: CentroidInfo,
    pub n_pixels: usize,
    pub n_empty_bins: usize,
    pub df_window: String,
    pub pixels: Vec<PixelSample>,
    pub bin_width_px: f64,
}

impl AnalysisParams {
    pub fn validate(&self) -> Result<(), String> {
        if self.supersample == 0 || self.supersample > 32 {
            return Err("supersample must be in 1..=32".into());
        }
        if self.window_half_bins < 8 {
            return Err("window_half_bins must be >= 8".into());
        }
        for r in &self.excluded_rows {
            let (_, lh) = self.roi.local_size();
            if *r >= lh {
                return Err(format!("excluded row {r} outside ROI height {lh}"));
            }
        }
        if self.pitch_mode == PitchMode::Manual {
            match self.manual_pitch_um {
                Some(p) if p > 0.0 && p.is_finite() => {}
                _ => return Err("manual pitch must be a positive number".into()),
            }
        }
        Ok(())
    }

    /// Canonical parameter fingerprint: stable JSON serialization then FNV-1a.
    pub fn fingerprint(&self) -> String {
        let canonical = serde_json::to_string(self).expect("params serialize");
        format!(
            "ss{k}x{d}xw{w}v{win}p{p}-{hash}",
            k = self.supersample,
            d = match self.derivative {
                DerivativeKernel::Central3 => "c3",
                DerivativeKernel::FivePoint => "f5",
                DerivativeKernel::SevenPoint => "s7",
            },
            w = self.window_half_bins,
            win = match self.spectral_window {
                SpectralWindow::Hann => "h",
                SpectralWindow::Rectangular => "r",
            },
            p = match self.pitch_mode {
                PitchMode::Auto => "auto",
                PitchMode::Manual => "manual",
                PitchMode::Ignore => "ignore",
            },
            hash = &crate::image::checksum_hex(canonical.as_bytes())[..10],
        )
    }
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let pos = q * (sorted.len() as f64 - 1.0);
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let frac = pos - lo as f64;
    sorted[lo] * (1.0 - frac) + sorted[hi] * frac
}

/// Detect the edge position in one local row by half-amplitude interpolation.
fn detect_row_edge(row_vals: &[f64]) -> Option<(f64, f64, f64)> {
    let mut sorted: Vec<f64> = row_vals.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let low = percentile(&sorted, 0.10);
    let high = percentile(&sorted, 0.90);
    let contrast = high - low;
    if contrast < 0.15 {
        return None;
    }
    let level = 0.5 * (low + high);
    // First crossing of `level` scanning u ascending.
    for u in 1..row_vals.len() {
        let (a, b) = (row_vals[u - 1], row_vals[u]);
        if (a <= level && b >= level) || (a >= level && b <= level) {
            if (b - a).abs() < 1e-12 {
                continue;
            }
            let frac = (level - a) / (b - a);
            // position in local u-CENTER coordinates
            return Some(((u - 1) as f64 + 0.5 + frac, low, high));
        }
    }
    None
}

fn stencil_dot<F: Fn(i64) -> Option<f64>>(stencil: &F, weights: &[(i64, f64)]) -> Option<f64> {
    weights
        .iter()
        .map(|(off, w)| stencil(*off).map(|v| v * w))
        .fold(Some(0.0), |acc, term| acc.zip(term).map(|(a, b)| a + b))
}

/// Theil-Sen line fit (robust to defect rows that survive auto exclusion).
fn theil_sen(points: &[(f64, f64)]) -> (f64, f64) {
    let mut slopes = Vec::new();
    for i in 0..points.len() {
        for j in (i + 1)..points.len() {
            let dv = points[j].0 - points[i].0;
            if dv.abs() > 1e-12 {
                slopes.push((points[j].1 - points[i].1) / dv);
            }
        }
    }
    slopes.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let slope = if slopes.is_empty() {
        0.0
    } else {
        percentile(&slopes, 0.5)
    };
    let mut intercepts: Vec<f64> = points.iter().map(|(v, u)| u - slope * v).collect();
    intercepts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let intercept = percentile(&intercepts, 0.5);
    (slope, intercept)
}

pub fn analyze(
    img: &GrayImage,
    bands: &[crate::image::PitchBand],
    params: &AnalysisParams,
) -> Result<AnalysisResult, String> {
    params.validate()?;
    params.roi.validate(img.width, img.height)?;
    let (lw, lh) = params.roi.local_size();

    let excluded: std::collections::HashSet<u32> = params.excluded_rows.iter().copied().collect();

    // ---- 1. choose the edge-crossing axis in the local frame ----
    // Scan a few lines in both directions; the edge is detected on the axis
    // whose lines have the larger median contrast. This makes analysis correct
    // after a 90/270 degree ROI rotation without resampling any pixels.
    let local = |u: u32, v: u32| {
        let (ox, oy) = params.roi.map_pixel(u, v);
        img.value01(ox, oy)
    };
    let line_contrast = |along_u: bool, fixed: u32| -> f64 {
        let vals: Vec<f64> = if along_u {
            (0..lw).map(|u| local(u, fixed)).collect()
        } else {
            (0..lh).map(|v| local(fixed, v)).collect()
        };
        let mut s = vals;
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        percentile(&s, 0.90) - percentile(&s, 0.10)
    };
    let probe = 5;
    let mut c_along_u = Vec::new();
    let mut c_along_v = Vec::new();
    for i in 0..probe {
        let vu = lh * i / probe;
        let vv = lw * i / probe;
        c_along_u.push(line_contrast(true, vu));
        c_along_v.push(line_contrast(false, vv));
    }
    c_along_u.sort_by(|a, b| a.partial_cmp(b).unwrap());
    c_along_v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med_u = percentile(&c_along_u, 0.5);
    let med_v = percentile(&c_along_v, 0.5);
    // swap=true: edge is crossed while scanning v, detected on lines u=const
    let swap = med_v > med_u;
    let (n_lines, n_across) = if swap { (lw, lh) } else { (lh, lw) };

    // ---- 2. per-line edge detection ----
    let mut rows = Vec::with_capacity(n_lines as usize);
    for line in 0..n_lines {
        // a "line" is a local row v unless axes are swapped (then a column u)
        let user_excluded = excluded.contains(&line);
        let vals: Vec<f64> = if swap {
            (0..lh).map(|v| local(line, v)).collect()
        } else {
            (0..lw).map(|u| local(u, line)).collect()
        };
        let (detected, low, high, reason, included) = match detect_row_edge(&vals) {
            Some((e, lo, hi)) if !user_excluded => (Some(e), lo, hi, String::new(), true),
            Some((e, lo, hi)) => (Some(e), lo, hi, "用户排除行".to_string(), false),
            None => {
                let mut s2 = vals.clone();
                s2.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let lo = percentile(&s2, 0.10);
                let hi = percentile(&s2, 0.90);
                let why = if user_excluded {
                    "用户排除行".to_string()
                } else {
                    format!("自动排除: 行对比度 {:.3} < 0.15", hi - lo)
                };
                (None, lo, hi, why, false)
            }
        };
        rows.push(RowEdge {
            row: line,
            detected_edge_u: detected,
            low,
            high,
            contrast: high - low,
            included,
            reason,
        });
    }

    // fit: across_center = intercept + slope * line_center, in CENTER coords
    let fit_points: Vec<(f64, f64)> = rows
        .iter()
        .filter(|r| r.included)
        .filter_map(|r| r.detected_edge_u.map(|e| (r.row as f64 + 0.5, e)))
        .collect();
    if fit_points.len() < 3 {
        return Err(format!(
            "可用边缘行仅 {} 行 (<3)，无法拟合边缘",
            fit_points.len()
        ));
    }
    let (slope, intercept) = theil_sen(&fit_points);

    let residuals: Vec<FitResidual> = fit_points
        .iter()
        .map(|(lc, ac)| FitResidual {
            row: (lc - 0.5).round() as u32,
            residual_px: ac - (intercept + slope * lc),
        })
        .collect();
    let rmse = (residuals
        .iter()
        .map(|r| r.residual_px * r.residual_px)
        .sum::<f64>()
        / residuals.len() as f64)
        .sqrt();
    let max_abs = residuals
        .iter()
        .map(|r| r.residual_px.abs())
        .fold(0.0, f64::max);
    let angle = slope.atan().to_degrees();
    let fit = EdgeFit {
        slope,
        intercept,
        angle_deg: angle,
        rmse_px: rmse,
        max_abs_residual_px: max_abs,
        n_rows: residuals.len(),
        residuals,
    };

    // ---- 3. pitch resolution across included lines ----
    let pitch = resolve_pitch(params, &params.roi, bands, &rows, swap, lw, lh);
    let (_lw, _lh) = (lw, lh);

    // ---- project included pixels onto the fitted edge normal ----
    // In (line, across) center coordinates the edge is across = m*line + b,
    // with normal n=(1,-m)/sqrt(1+m^2) pointing to the across-positive side.
    // Mapping back to local (u,v) preserves the same physical normal.
    let norm = (1.0 + slope * slope).sqrt();
    // Polarity: signed distance must increase toward the BRIGHT side. Measure
    // mean brightness on the two sides of the fitted edge along `across`.
    let mut bright_sum = 0.0f64;
    let mut bright_n = 0u32;
    let mut dark_sum = 0.0f64;
    let mut dark_n = 0u32;
    for rr in &rows {
        if !rr.included {
            continue;
        }
        let line_c = rr.row as f64 + 0.5;
        let edge_across = intercept + slope * line_c;
        for across in 0..n_across {
            let (u, v) = if swap {
                (rr.row, across)
            } else {
                (across, rr.row)
            };
            let (ox, oy) = params.roi.map_pixel(u, v);
            let val = img.value01(ox, oy);
            if across as f64 + 0.5 > edge_across {
                bright_sum += val;
                bright_n += 1;
            } else {
                dark_sum += val;
                dark_n += 1;
            }
        }
    }
    let polarity = if bright_n > 0
        && dark_n > 0
        && bright_sum / (bright_n as f64) < dark_sum / (dark_n as f64)
    {
        -1.0
    } else {
        1.0
    };
    let mut samples: Vec<PixelSample> = Vec::new();
    for rr in &rows {
        if !rr.included {
            continue;
        }
        let line = rr.row;
        let line_c = line as f64 + 0.5;
        let edge_across = intercept + slope * line_c;
        for across in 0..n_across {
            let (u, v) = if swap { (line, across) } else { (across, line) };
            let (ox, oy) = params.roi.map_pixel(u, v);
            let (cx, cy) = params.roi.map_center(u, v);
            let across_c = across as f64 + 0.5;
            let d = polarity * (across_c - edge_across) / norm;
            samples.push(PixelSample {
                u,
                v,
                ox,
                oy,
                center_x: cx,
                center_y: cy,
                value01: img.value01(ox, oy),
                signed_distance_px: d,
                bin_index: 0,
            });
        }
    }
    let n_pixels = samples.len();

    // ---- 4. supersampled ESF bins: bin k spans [k/k, (k+1)/k) centered at edge ----
    let k = params.supersample as i64;
    let bin_w = 1.0 / params.supersample as f64;
    let dmin = samples
        .iter()
        .map(|s| s.signed_distance_px)
        .fold(f64::INFINITY, f64::min);
    let dmax = samples
        .iter()
        .map(|s| s.signed_distance_px)
        .fold(f64::NEG_INFINITY, f64::max);
    // bins cover [floor(dmin)-1, ceil(dmax)+1] so edge-adjacent empty bins exist
    let k_lo = ((dmin.floor() as i64) - 1) * k;
    let k_hi = ((dmax.ceil() as i64) + 1) * k;
    let n_bins = (k_hi - k_lo + 1) as usize;
    let mut sums = vec![0.0f64; n_bins];
    let mut counts = vec![0u32; n_bins];
    for s in samples.iter_mut() {
        // bin whose center is closest on the negative grid: index = floor(d*k + 0.5)
        let idx = (s.signed_distance_px * params.supersample as f64 + 0.5).floor() as i64;
        s.bin_index = idx;
        if idx < k_lo || idx > k_hi {
            continue;
        }
        let slot = (idx - k_lo) as usize;
        sums[slot] += s.value01;
        counts[slot] += 1;
    }
    let bins: Vec<EsfBin> = (k_lo..=k_hi)
        .map(|idx| {
            let slot = (idx - k_lo) as usize;
            let center = (idx as f64 + 0.5) * bin_w;
            EsfBin {
                index: idx,
                center_px: center,
                count: counts[slot],
                // IMPORTANT: empty bins stay null, never copied/filled.
                value: if counts[slot] == 0 {
                    None
                } else {
                    Some(sums[slot] / counts[slot] as f64)
                },
            }
        })
        .collect();
    let n_empty = bins.iter().filter(|b| b.value.is_none()).count();

    // ---- 5. LSF by finite differences (null propagates through the stencil) ----
    let hw = params.derivative.half_width() as i64;
    let mut lsf: Vec<LsfPoint> = Vec::with_capacity(n_bins);
    for (pos, bin) in bins.iter().enumerate() {
        let idx = bin.index;
        let stencil = |off: i64| -> Option<f64> {
            let p = pos as i64 + off;
            if p < 0 || p >= n_bins as i64 {
                None
            } else {
                bins[p as usize].value
            }
        };
        // Coefficients (a*[+1] + b*[-1] ...) / (denom * bin_width).
        // Any null ESF bin inside the stencil -> null LSF bin (never filled).
        let deriv = match params.derivative {
            DerivativeKernel::Central3 => {
                let weights: [(i64, f64); 2] = [(1, 1.0), (-1, -1.0)];
                let den = 2.0;
                stencil_dot(&stencil, &weights).map(|v| v / (den * bin_w))
            }
            DerivativeKernel::FivePoint => {
                let weights = [(1, 8.0), (-1, -8.0), (2, -1.0), (-2, 1.0)];
                stencil_dot(&stencil, &weights).map(|v| v / (12.0 * bin_w))
            }
            DerivativeKernel::SevenPoint => {
                let weights = [
                    (1, 45.0),
                    (-1, -45.0),
                    (2, -9.0),
                    (-2, 9.0),
                    (3, 1.0),
                    (-3, -1.0),
                ];
                stencil_dot(&stencil, &weights).map(|v| v / (60.0 * bin_w))
            }
        };
        let _ = hw;
        lsf.push(LsfPoint {
            index: idx,
            center_px: bin.center_px,
            value: deriv,
        });
    }

    // ---- 6. trim to contiguous non-null core around the LSF peak, window, DFT ----
    let (mtf, df_window_name, core_range) = compute_mtf(&lsf, bin_w, params, pitch.pitch_um)?;
    let (peak_idx, centroid, fwhm) = lsf_shape(&lsf, core_range, bin_w);
    let nyquist = nyquist_behavior(&mtf);
    let crossings = find_crossings(&mtf, pitch.pitch_um);
    let mtf50 = crossings.iter().find(|c| c.direction == "down").cloned();
    let crossing_rule = "MTF50 取主瓣沿频率升高方向第一次下穿 0.5 的交点（线性插值）；\
                         其余所有上穿/下穿交点按频率顺序列出并标注波瓣编号。"
        .to_string();

    let params_json = serde_json::to_string(params).unwrap();
    Ok(AnalysisResult {
        params_fingerprint: params.fingerprint(),
        params_json,
        rows,
        fit,
        pitch,
        bins,
        lsf,
        mtf,
        crossings,
        mtf50,
        crossing_rule,
        nyquist,
        centroid: CentroidInfo {
            centroid_px: centroid,
            peak_index: peak_idx,
            peak_px: (peak_idx as f64 + 0.5) * bin_w,
            fwhm_px: fwhm,
        },
        n_pixels,
        n_empty_bins: n_empty,
        df_window: df_window_name,
        pixels: samples,
        bin_width_px: bin_w,
    })
}

fn resolve_pitch(
    params: &AnalysisParams,
    roi: &Roi,
    bands: &[crate::image::PitchBand],
    rows: &[RowEdge],
    swap: bool,
    _lw: u32,
    _lh: u32,
) -> PitchResolution {
    let _ = swap;
    match params.pitch_mode {
        PitchMode::Manual => {
            let p = params.manual_pitch_um.unwrap();
            PitchResolution {
                mode: "manual".into(),
                pitch_um: Some(p),
                resolved: true,
                note: format!("手工指定像素间距 {p} µm"),
                bands: vec![],
            }
        }
        PitchMode::Ignore => PitchResolution {
            mode: "ignore".into(),
            pitch_um: None,
            resolved: false,
            note: "明确忽略像素间距：只显示 cycles/pixel".into(),
            bands: vec![],
        },
        PitchMode::Auto => {
            // Collect bands touched by included local rows. Under 90/270 the
            // local v axis moves along original *columns*; pitch bands are
            // horizontal row bands, so a single local row crosses every band.
            let (lw, _lh) = roi.local_size();
            let mut hit: Vec<usize> = Vec::new();
            for r in rows.iter().filter(|r| r.included) {
                let v = r.row;
                for u in 0..lw {
                    let (_ox, oy) = roi.map_pixel(u, v);
                    for (bi, b) in bands.iter().enumerate() {
                        if oy >= b.y0 && oy < b.y1 && !hit.contains(&bi) {
                            hit.push(bi);
                        }
                    }
                }
            }
            hit.sort_unstable();
            if hit.len() == 1 {
                let b = &bands[hit[0]];
                match b.pitch_um {
                    Some(p) => PitchResolution {
                        mode: "auto".into(),
                        pitch_um: Some(p),
                        resolved: true,
                        note: format!("ROI 完全落在第 {} 段，像素间距 {p} µm", hit[0] + 1),
                        bands: hit,
                    },
                    None => PitchResolution {
                        mode: "auto".into(),
                        pitch_um: None,
                        resolved: false,
                        note: "该段图像未提供像素间距：只显示 cycles/pixel".into(),
                        bands: hit,
                    },
                }
            } else if hit.is_empty() {
                PitchResolution {
                    mode: "auto".into(),
                    pitch_um: None,
                    resolved: false,
                    note: "ROI 内无可用行".into(),
                    bands: hit,
                }
            } else {
                let pitches: Vec<Option<f64>> = hit.iter().map(|i| bands[*i].pitch_um).collect();
                let same = pitches.iter().all(|p| p.is_some() && (*p == pitches[0]));
                if same {
                    PitchResolution {
                        mode: "auto".into(),
                        pitch_um: pitches[0],
                        resolved: true,
                        note: format!("横跨 {} 段但像素间距一致", hit.len()),
                        bands: hit,
                    }
                } else {
                    PitchResolution {
                        mode: "auto".into(),
                        pitch_um: None,
                        resolved: false,
                        note: format!(
                            "ROI 横跨两段不同像素间距（{:?} µm），频率仅以 cycles/pixel 显示；\
                             请裁切到单一段落或手工指定",
                            pitches
                                .iter()
                                .map(|p| p.map(|v| format!("{v}")).unwrap_or_else(|| "未知".into()))
                                .collect::<Vec<_>>()
                        ),
                        bands: hit,
                    }
                }
            }
        }
    }
}

fn compute_mtf(
    lsf: &[LsfPoint],
    bin_w: f64,
    params: &AnalysisParams,
    pitch_um: Option<f64>,
) -> Result<(Vec<MtfPoint>, String, (i64, i64)), String> {
    // largest contiguous non-null segment of the LSF (derivative)
    let n = lsf.len() as i64;
    let mut best: Option<(i64, i64)> = None;
    let mut cur_start: Option<i64> = None;
    for i in 0..n {
        if lsf[i as usize].value.is_some() {
            if cur_start.is_none() {
                cur_start = Some(i);
            }
        } else if let Some(s) = cur_start.take() {
            let cand = (s, i - 1);
            best = Some(match best {
                None => cand,
                Some(b) if cand.1 - cand.0 > b.1 - b.0 => cand,
                Some(b) => b,
            });
        }
    }
    if let Some(s) = cur_start {
        let cand = (s, n - 1);
        best = Some(match best {
            None => cand,
            Some(b) if cand.1 - cand.0 > b.1 - b.0 => cand,
            Some(b) => b,
        });
    }
    let (s0, s1) = best.ok_or("LSF 全部为空箱（微分核窗口内存在缺失），无法计算 MTF")?;

    // Peak index within the segment; trim symmetrically to window_half_bins
    // around the peak (clamped to the segment).
    let mut peak_local = 0i64;
    let mut peak_val = f64::MIN;
    for i in s0..=s1 {
        let v = lsf[i as usize].value.unwrap();
        if v > peak_val {
            peak_val = v;
            peak_local = i;
        }
    }
    let half = params.window_half_bins as i64;
    let w0 = (peak_local - half).max(s0);
    let w1 = (peak_local + half).min(s1);

    let len = (w1 - w0 + 1) as usize;
    let mut win = vec![0.0f64; len];
    let m = len.saturating_sub(1) as f64;
    let mut core_sum = 0.0;
    for (j, i) in (w0..=w1).enumerate() {
        let v = lsf[i as usize].value.unwrap();
        let hann = 0.5 - 0.5 * (std::f64::consts::TAU * j as f64 / m).cos();
        win[j] = v * hann;
        core_sum += v;
    }
    if core_sum.abs() < 1e-12 {
        return Err("LSF 直流分量为 0，无法归一化 MTF".into());
    }

    // naive DFT is plenty for N <= ~160 bins; report up to 0.6 c/p.
    let nfft = len;
    let f_step = 1.0 / (nfft as f64 * bin_w);
    let f_max = 0.6;
    let n_out = (f_max / f_step).floor() as usize + 1;
    let mut mtf = Vec::with_capacity(n_out);
    for q in 0..n_out {
        let mut re = 0.0f64;
        let mut im = 0.0f64;
        let ang = std::f64::consts::TAU * q as f64 / nfft as f64;
        for j in 0..nfft {
            let phase = ang * j as f64;
            re += win[j] * phase.cos();
            im += win[j] * (-phase).sin();
        }
        let mag = re.hypot(im);
        mtf.push(mag);
    }
    let dc = mtf[0];
    let lpmm = |f: f64| pitch_um.map(|p| f * 1000.0 / p);
    let points: Vec<MtfPoint> = mtf
        .iter()
        .enumerate()
        .map(|(q, m)| MtfPoint {
            f_cpp: q as f64 * f_step,
            f_lpmm: lpmm(q as f64 * f_step),
            mtf: m / dc,
        })
        .collect();
    let wlabel = if matches!(params.spectral_window, SpectralWindow::Hann) {
        "Hann 窗"
    } else {
        "矩形窗（保留旁瓣证据）"
    };
    let note = format!(
        "{wlabel}，{len} 个连续非空箱 (箱索引 {}..{}, 箱宽 {:.4}px), Δf={:.5} c/p",
        lsf[w0 as usize].index, lsf[w1 as usize].index, bin_w, f_step
    );
    Ok((points, note, (w0, w1)))
}

fn lsf_shape(lsf: &[LsfPoint], core: (i64, i64), bin_w: f64) -> (i64, f64, Option<f64>) {
    let (w0, w1) = core;
    let vals: Vec<(i64, f64)> = (w0..=w1)
        .map(|i| (lsf[i as usize].index, lsf[i as usize].value.unwrap()))
        .collect();
    let mut peak_idx = vals[0].0;
    let mut peak_v = f64::MIN;
    for (idx, v) in &vals {
        if *v > peak_v {
            peak_v = *v;
            peak_idx = *idx;
        }
    }
    // signed centroid (handles ringing negatives)
    let mut num = 0.0;
    let mut den = 0.0;
    for (idx, v) in &vals {
        let c = (*idx as f64 + 0.5) * bin_w;
        num += c * v;
        den += v;
    }
    let centroid = num / den;
    // FWHM of the main lobe: half-peak crossings immediately left/right of peak
    let half = peak_v / 2.0;
    let pos = vals.iter().position(|(i, _)| *i == peak_idx).unwrap();
    let mut left: Option<f64> = None;
    for j in (1..=pos).rev() {
        let v0 = vals[j].1;
        let v1 = vals[j - 1].1;
        if (v0 - half) * (v1 - half) <= 0.0 && v0 != v1 {
            let t = (half - v0) / (v1 - v0);
            let c0 = (vals[j].0 as f64 + 0.5) * bin_w;
            let c1 = (vals[j - 1].0 as f64 + 0.5) * bin_w;
            left = Some(c0 + t * (c1 - c0));
            break;
        }
    }
    let mut right: Option<f64> = None;
    for j in pos..vals.len() - 1 {
        let v0 = vals[j].1;
        let v1 = vals[j + 1].1;
        if (v0 - half) * (v1 - half) <= 0.0 && v0 != v1 {
            let t = (half - v0) / (v1 - v0);
            let c0 = (vals[j].0 as f64 + 0.5) * bin_w;
            let c1 = (vals[j + 1].0 as f64 + 0.5) * bin_w;
            right = Some(c0 + t * (c1 - c0));
            break;
        }
    }
    let fwhm = left.zip(right).map(|(l, r)| (r - l).abs());
    (peak_idx, centroid, fwhm)
}

fn nyquist_behavior(mtf: &[MtfPoint]) -> NyquistBehavior {
    let at = |f0: f64, f1: f64| -> f64 {
        let sel: Vec<f64> = mtf
            .iter()
            .filter(|p| p.f_cpp >= f0 && p.f_cpp <= f1)
            .map(|p| p.mtf)
            .collect();
        if sel.is_empty() {
            f64::NAN
        } else {
            sel.iter().sum::<f64>() / sel.len() as f64
        }
    };
    let nyq = interpolate_mtf(mtf, 0.5);
    let hn = interpolate_mtf(mtf, 0.25);
    let pre = at(0.40, 0.50);
    let post = at(0.50, 0.60);
    NyquistBehavior {
        mtf_at_nyquist: nyq,
        mtf_at_half_nyquist: hn,
        mean_04_05: pre,
        mean_05_06: post,
        rises_after_nyquist: post > pre * 1.05,
        alias_ratio: if pre > 0.0 { post / pre } else { f64::INFINITY },
    }
}

fn interpolate_mtf(mtf: &[MtfPoint], f: f64) -> f64 {
    if f <= mtf[0].f_cpp {
        return mtf[0].mtf;
    }
    for w in mtf.windows(2) {
        if w[0].f_cpp <= f && f <= w[1].f_cpp {
            let t = (f - w[0].f_cpp) / (w[1].f_cpp - w[0].f_cpp);
            return w[0].mtf + t * (w[1].mtf - w[0].mtf);
        }
    }
    mtf.last().unwrap().mtf
}

fn find_crossings(mtf: &[MtfPoint], pitch_um: Option<f64>) -> Vec<Crossing> {
    let lpmm = |f: f64| pitch_um.map(|p| f * 1000.0 / p);
    let mut out = Vec::new();
    // lobes counted by sign changes in the underlying LSF-DFT imaginary part
    // are not available here; approximate lobes by local maxima of |MTF|:
    // crossing segment index increments per crossing, lobe increments whenever
    // an upward crossing starts a new side lobe.
    let mut lobe = 0usize;
    for (i, w) in mtf.windows(2).enumerate() {
        let (a, b) = (w[0].mtf, w[1].mtf);
        if a.is_nan() || b.is_nan() || a == b {
            continue;
        }
        if (a - 0.5) * (b - 0.5) < 0.0 {
            let t = (0.5 - a) / (b - a);
            let f = w[0].f_cpp + t * (w[1].f_cpp - w[0].f_cpp);
            let up = b > a;
            if up {
                lobe += 1;
            }
            out.push(Crossing {
                f_cpp: f,
                f_lpmm: lpmm(f),
                segment_index: i,
                direction: if up { "up".into() } else { "down".into() },
                lobe,
            });
        }
    }
    out
}
