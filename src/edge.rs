//! Per-scan-line sub-pixel edge localization and least-squares edge fitting.

use crate::geometry::GrayImage;
use crate::model::{EdgeFit, FitKind, Orientation, RowObservation};

/// Localize the transition along one scan profile using a mid-level linear
/// interpolation after locating the steepest sample-to-sample gradient.
///
/// `sign` selects the expected polarity: 1.0 for dark-to-bright profiles,
/// -1.0 for bright-to-dark, 0.0 to accept whichever polarity dominates.
pub fn locate_crossing(profile: &[f64], sign: f64) -> Option<f64> {
    let w = profile.len();
    if w < 6 {
        return None;
    }
    // Steepest gradient index `e` (between samples e and e+1).
    let mut best_e = 0usize;
    let mut best_g = 0.0f64;
    for e in 0..w - 1 {
        let g = (profile[e + 1] - profile[e]) * sign;
        if g > best_g {
            best_g = g;
            best_e = e;
        }
    }
    if best_g <= 1e-6 {
        return None;
    }
    // Plateau estimates, staying at least one sample off the transition.
    let left_end = best_e.saturating_sub(1).max(1);
    let right_start = (best_e + 2).min(w - 1);
    if left_end < 1 || right_start > w - 1 || right_start <= left_end {
        return None;
    }
    let lo = profile[..left_end].iter().sum::<f64>() / left_end as f64;
    let hi = profile[right_start..].iter().sum::<f64>() / (w - right_start) as f64;
    if (hi - lo).abs() < 0.05 {
        return None;
    }
    let level = (lo + hi) / 2.0;
    let (p0, p1) = (profile[best_e], profile[best_e + 1]);
    if (p1 - p0).abs() < 1e-12 {
        return Some(best_e as f64 + 0.5);
    }
    let t = (level - p0) / (p1 - p0);
    if !(0.0..=1.0).contains(&t) {
        return Some(best_e as f64 + 0.5);
    }
    Some(best_e as f64 + t)
}

fn ols(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    let n = points.len() as f64;
    if n < 2.0 {
        return None;
    }
    let sx = points.iter().map(|p| p.0).sum::<f64>();
    let sy = points.iter().map(|p| p.1).sum::<f64>();
    let sxx = points.iter().map(|p| p.0 * p.0).sum::<f64>();
    let sxy = points.iter().map(|p| p.0 * p.1).sum::<f64>();
    let denom = n * sxx - sx * sx;
    if denom.abs() < 1e-12 {
        return None;
    }
    let slope = (n * sxy - sx * sy) / denom;
    let intercept = (sy - slope * sx) / n;
    Some((intercept, slope))
}

fn fit_vertical(img: &GrayImage, excluded: &[i64]) -> Option<EdgeFit> {
    let mut obs: Vec<(f64, f64)> = Vec::new(); // (y-index, x-crossing)
    let mut rows = Vec::with_capacity(img.height);
    // Polarity from whole-frame column means.
    let col_sum = |c| (0..img.height).map(|r| img.at(c, r)).sum::<f64>() / img.height as f64;
    let sign = (col_sum(img.width - 1) - col_sum(0)).signum();
    let excluded = excluded.iter().copied().collect::<std::collections::HashSet<_>>();
    for r in 0..img.height as i64 {
        let prof: Vec<f64> = (0..img.width).map(|c| img.at(c, r as usize)).collect();
        let crossing = locate_crossing(&prof, sign);
        if crossing.is_some() && !excluded.contains(&r) {
            obs.push((r as f64, crossing.unwrap()));
        }
        rows.push((r, crossing));
    }
    let (intercept, slope) = ols(&obs)?;
    finalize(FitKind::Vertical, intercept, slope, rows, excluded)
}

fn fit_horizontal(img: &GrayImage, excluded: &[i64]) -> Option<EdgeFit> {
    let mut obs: Vec<(f64, f64)> = Vec::new(); // (x-index, y-crossing)
    let mut rows = Vec::with_capacity(img.width);
    let row_sum = |r| (0..img.width).map(|c| img.at(c, r)).sum::<f64>() / img.width as f64;
    let sign = (row_sum(img.height - 1) - row_sum(0)).signum();
    let excluded = excluded.iter().copied().collect::<std::collections::HashSet<_>>();
    for c in 0..img.width as i64 {
        let prof: Vec<f64> = (0..img.height).map(|r| img.at(c as usize, r)).collect();
        let crossing = locate_crossing(&prof, sign);
        if crossing.is_some() && !excluded.contains(&c) {
            obs.push((c as f64, crossing.unwrap()));
        }
        rows.push((c, crossing));
    }
    let (intercept, slope) = ols(&obs)?;
    finalize(FitKind::Horizontal, intercept, slope, rows, excluded)
}

fn finalize(
    kind: FitKind,
    intercept: f64,
    slope: f64,
    raw: Vec<(i64, Option<f64>)>,
    excluded: std::collections::HashSet<i64>,
) -> Option<EdgeFit> {
    let mut used = 0usize;
    let mut sse = 0.0f64;
    let mut max_abs = 0.0f64;
    let mut out = Vec::with_capacity(raw.len());
    for (idx, crossing) in raw {
        let predicted = match kind {
            FitKind::Vertical => intercept + slope * idx as f64,
            FitKind::Horizontal => intercept + slope * idx as f64,
        };
        let is_excl = excluded.contains(&idx);
        let residual = crossing.map(|x| x - predicted);
        if crossing.is_some() && !is_excl {
            used += 1;
            let e = residual.unwrap();
            sse += e * e;
            max_abs = max_abs.max(e.abs());
        }
        out.push(RowObservation {
            index: idx,
            crossing,
            detected: crossing.is_some(),
            excluded: is_excl,
            predicted: Some(predicted),
            residual,
        });
    }
    if used < 2 {
        return None;
    }
    let rms = (sse / used as f64).sqrt();
    let angle_deg = slope.atan().to_degrees();
    Some(EdgeFit {
        kind,
        intercept,
        slope,
        angle_deg,
        used_samples: used,
        rms_residual: rms,
        max_abs_residual: max_abs,
        rows: out,
    })
}

/// Fit the edge orientation requested. For `Auto`, both fits are attempted and
/// the one with the lower RMS residual (requiring |angle| < 30°) is selected.
pub fn fit_edge(img: &GrayImage, orientation: Orientation, excluded: &[i64]) -> Option<EdgeFit> {
    match orientation {
        Orientation::Vertical => fit_vertical(img, excluded),
        Orientation::Horizontal => fit_horizontal(img, excluded),
        Orientation::Auto => {
            let v = fit_vertical(img, excluded).filter(|f| f.angle_deg.abs() < 30.0);
            let h = fit_horizontal(img, excluded).filter(|f| f.angle_deg.abs() < 30.0);
            match (v, h) {
                (Some(vf), Some(hf)) => {
                    if hf.rms_residual < vf.rms_residual {
                        Some(hf)
                    } else {
                        Some(vf)
                    }
                }
                (Some(vf), None) => Some(vf),
                (None, Some(hf)) => Some(hf),
                (None, None) => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_of_perfect_step() {
        let prof = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let c = locate_crossing(&prof, 1.0).unwrap();
        assert!((c - 3.5).abs() < 1e-9);
        let prof2 = vec![1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let c2 = locate_crossing(&prof2, -1.0).unwrap();
        assert!((c2 - 2.5).abs() < 1e-9);
    }

    #[test]
    fn fit_perfect_slanted_edge() {
        let mut img = GrayImage::filled(20, 16, 0.0);
        for r in 0..16 {
            let edge_x = 4.0 + 0.2 * r as f64;
            for c in 0..20 {
                if c as f64 + 0.5 > edge_x {
                    img.set(c, r, 1.0);
                }
            }
        }
        let fit = fit_edge(&img, Orientation::Auto, &[]).unwrap();
        assert_eq!(fit.kind, FitKind::Vertical);
        assert!((fit.slope - 0.2).abs() < 1e-6, "slope {}", fit.slope);
        assert!(fit.rms_residual < 1e-6);
        assert_eq!(fit.used_samples, 16);
    }
}
