//! Full slanted-edge pipeline: pixels -> edge fit -> oversampled ESF ->
//! differentiated LSF -> windowed FFT -> normalized MTF / MTF50.
//!
//! Missing-data rule: an ESF bin with zero contributing samples is kept as
//! `None`. Adjacent bins are never copied or interpolated into it. LSF points
//! whose differentiation taps touch a missing bin are also `None`. The FFT uses
//! the longest contiguous valid LSF region around the edge.

use std::collections::{BTreeMap, HashSet};

use crate::edge::fit_edge;
use crate::fftx::real_spectrum;
use crate::geometry::{extract_roi, GrayImage, RoiTransform};
use crate::model::*;

const MTF50_RULE: &str =
    "MTF50 = 第一次从上方穿越 0.5 的下降向交点（从 f=0 起逐段线性插值扫描）；其余全部 0.5 交点按方向单独列出。";

fn signed_distance(col: i64, row: i64, fit: &EdgeFit) -> f64 {
    match fit.kind {
        FitKind::Vertical => col as f64 + 0.5 - (fit.intercept + fit.slope * row as f64),
        FitKind::Horizontal => row as f64 + 0.5 - (fit.intercept + fit.slope * col as f64),
    }
}

fn build_esf(img: &GrayImage, fit: &EdgeFit, k: i64, excluded: &[i64]) -> Vec<EsfBin> {
    let excluded: HashSet<i64> = excluded.iter().copied().collect();
    let scan_len = match fit.kind {
        FitKind::Vertical => img.width as i64,
        FitKind::Horizontal => img.height as i64,
    };
    let (d_min, d_max) = edge_distance_bounds(fit, img);
    let max_abs = d_min.abs().max(d_max.abs());
    let span = max_abs + (scan_len as f64) / 2.0;
    let max_bin = (span * k as f64).ceil() as i64 + 1;

    let mut sums: BTreeMap<i64, (f64, u32)> = BTreeMap::new();
    for r in 0..img.height as i64 {
        for c in 0..img.width as i64 {
            let scan_idx = match fit.kind {
                FitKind::Vertical => r,
                FitKind::Horizontal => c,
            };
            if excluded.contains(&scan_idx) {
                continue;
            }
            let d = signed_distance(c, r, fit);
            let b = (d * k as f64).floor() as i64;
            if b < -max_bin || b > max_bin {
                continue;
            }
            let e = sums.entry(b).or_insert((0.0, 0));
            e.0 += img.at(c as usize, r as usize);
            e.1 += 1;
        }
    }
    let mut out = Vec::new();
    for b in -max_bin..=max_bin {
        let mean = sums.get(&b).map(|(s, n)| s / *n as f64);
        out.push(EsfBin {
            index: b,
            distance: (b as f64 + 0.5) / k as f64,
            count: sums.get(&b).map(|x| x.1).unwrap_or(0),
            mean,
        });
    }
    out
}

/// Minimum/maximum signed distance of observed scan lines at the edge itself.
fn edge_distance_bounds(fit: &EdgeFit, img: &GrayImage) -> (f64, f64) {
    match fit.kind {
        FitKind::Vertical => {
            let ys = 0..img.height as i64;
            let ds: Vec<f64> = ys
                .map(|r| (fit.intercept + fit.slope * r as f64) - (r as f64 * 0.0) - 0.0)
                .collect();
            // Distance of edge x-coordinate from the column-center lattice
            // varies with row; compute fractional offsets directly.
            let mut vals = Vec::new();
            for r in 0..img.height as i64 {
                let edge_x = fit.intercept + fit.slope * r as f64;
                vals.push(edge_x - edge_x.floor() - 0.5);
            }
            let (a, b) = minmax(&vals);
            let _ = ds;
            (a, b)
        }
        FitKind::Horizontal => {
            let mut vals = Vec::new();
            for c in 0..img.width as i64 {
                let edge_y = fit.intercept + fit.slope * c as f64;
                vals.push(edge_y - edge_y.floor() - 0.5);
            }
            let (a, b) = minmax(&vals);
            (a, b)
        }
    }
}

fn minmax(v: &[f64]) -> (f64, f64) {
    let mut a = f64::INFINITY;
    let mut b = f64::NEG_INFINITY;
    for x in v {
        a = a.min(*x);
        b = b.max(*x);
    }
    (a, b)
}

fn build_lsf(esf: &[EsfBin], k: i64, m: i64) -> Vec<LsfPoint> {
    let by: BTreeMap<i64, f64> = esf
        .iter()
        .filter_map(|b| b.mean.map(|v| (b.index, v)))
        .collect();
    esf.iter()
        .map(|b| {
            let mut acc = 0.0f64;
            let mut ok = true;
            for t in 1..=m {
                let rp = by.get(&(b.index + t));
                let rn = by.get(&(b.index - t));
                match (rp, rn) {
                    (Some(p), Some(n)) => acc += p - n,
                    _ => ok = false,
                }
            }
            LsfPoint {
                index: b.index,
                distance: b.distance,
                value: if ok { Some(acc) } else { None },
            }
        })
        .collect()
}
