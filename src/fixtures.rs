//! Three deterministic, hand-designed fixtures (fixed seeds). Regenerating them
//! on any machine yields byte-identical images; the page and README publish
//! their FNV-1a checksums.
//!
//! A: `bad_rows`    - known pitch, four bad rows (white / black / flat) to exclude.
//! B: `dual_pitch`  - top and bottom halves use different pixel pitch.
//! C: `ringing_edge`- unknown pitch, edge with ringing => MTF crosses 0.5 several times.

use crate::image::{clamp255, norm_cdf, GrayImage, PitchBand, Rng};

pub struct Fixture {
    pub key: &'static str,
    pub title: &'static str,
    pub width: u32,
    pub height: u32,
    pub pitch_bands: Vec<PitchBand>,
    pub note: &'static str,
    pub image: GrayImage,
}

/// Ideal slanted-edge ESF value in 0..=1. `d` is signed pixel distance from the
/// edge, positive into the bright side.
fn ideal_edge(d: f64, sigma: f64, low: f64, high: f64) -> f64 {
    low + (high - low) * norm_cdf(d, sigma)
}

/// Edge x-position (pixel-center coordinate) for a given row center `yc`.
fn edge_x(yc: f64, cx: f64, cy: f64, slope: f64) -> f64 {
    cx + slope * (yc - cy)
}

/// A: known pitch 5.0um; four bad rows.
fn gen_bad_rows() -> Fixture {
    let (w, h) = (96u32, 72u32);
    let (cx, cy, slope, sigma) = (48.0, 36.0, 0.12, 1.1);
    let (low, high) = (20.0 / 255.0, 230.0 / 255.0);
    let mut rng = Rng::new(0x0123_4567_89ab_cdef);
    let noise_sigma = 1.4 / 255.0;
    let bad: Vec<(u32, u8)> = vec![
        (13, 255), // saturated white scratch row
        (47, 0),
        (48, 0),   // double black defect row
        (60, 128), // flat mid-gray row (no edge)
    ];
    let mut data = vec![0u8; (w * h) as usize];
    for y in 0..h {
        let yc = y as f64 + 0.5;
        let ex = edge_x(yc, cx, cy, slope);
        for x in 0..w {
            let d = x as f64 + 0.5 - ex;
            let v = ideal_edge(d, sigma, low, high) + noise_sigma * rng.unit();
            data[(y * w + x) as usize] = clamp255(v);
        }
    }
    for &(y, val) in &bad {
        for x in 0..w {
            data[(y * w + x) as usize] = val;
        }
    }
    Fixture {
        key: "bad_rows",
        title: "带坏行刃边 (pitch 5.0µm)",
        width: w,
        height: h,
        pitch_bands: vec![PitchBand {
            y0: 0,
            y1: h,
            pitch_um: Some(5.0),
        }],
        note: "第13行全白、第47/48行全黑、第60行中灰无边缘；分析时应排除。",
        image: GrayImage::new(w, h, data),
    }
}

/// B: two bands with different pitch; sharpness in pixels identical on purpose
/// so lp/mm curves differ only because of the pitch convention.
fn gen_dual_pitch() -> Fixture {
    let (w, h) = (112u32, 96u32);
    let split = 48u32;
    let (cx, cy, slope, sigma) = (56.0, 48.0, 0.10, 1.2);
    let (low, high) = (24.0 / 255.0, 228.0 / 255.0);
    let mut rng = Rng::new(0x9abc_def0_1234_5678);
    let noise_sigma = 0.8 / 255.0;
    let mut data = vec![0u8; (w * h) as usize];
    for y in 0..h {
        let yc = y as f64 + 0.5;
        let ex = edge_x(yc, cx, cy, slope);
        for x in 0..w {
            let d = x as f64 + 0.5 - ex;
            let v = ideal_edge(d, sigma, low, high) + noise_sigma * rng.unit();
            data[(y * w + x) as usize] = clamp255(v);
        }
    }
    // A visible seam at y=48: thin marker column block so the boundary is
    // visible in the UI; does not intersect the edge path at the marker itself.
    for x in 0..6 {
        for y in split..h {
            data[(y * w + x) as usize] = 0;
        }
    }
    Fixture {
        key: "dual_pitch",
        title: "双像素间距刃边 (3.2µm / 6.4µm)",
        width: w,
        height: h,
        pitch_bands: vec![
            PitchBand {
                y0: 0,
                y1: split,
                pitch_um: Some(3.2),
            },
            PitchBand {
                y0: split,
                y1: h,
                pitch_um: Some(6.4),
            },
        ],
        note: "上半段(行0..48) pitch=3.2µm，下半段(行48..96) pitch=6.4µm；横跨两段时间距不确定。",
        image: GrayImage::new(w, h, data),
    }
}

/// C: gaussian edge plus a damped sinusoidal ESF term; unknown pitch.
/// The ringing term makes the MTF magnitude rise again after its first fall,
/// so the 0.5 level is crossed multiple times.
fn gen_ringing() -> Fixture {
    let (w, h) = (96u32, 80u32);
    let (cx, cy, slope, sigma) = (48.0, 40.0, 0.08, 0.7);
    let (low, high) = (30.0 / 255.0, 235.0 / 255.0);
    // A weak negative narrow lobe offset ~4.6px from the main LSF models
    // damped edge ringing (ESF overshoot/undershoot). After pixel integration,
    // supersampled binning and any of the three derivative kernels, the MTF
    // still dips below 0.5 near 0.39 c/p, rises above it near 0.42 c/p, then
    // falls below again near 0.47 c/p -> the 0.5 level is crossed 3 times.
    let lobe_amp = -0.40;
    let lobe_sigma = 0.25;
    let lobe_offset = 4.6;
    let mut rng = Rng::new(0x1357_2468_ace0_bdf9);
    let noise_sigma = 0.4 / 255.0;
    let mut data = vec![0u8; (w * h) as usize];
    for y in 0..h {
        let yc = y as f64 + 0.5;
        let ex = edge_x(yc, cx, cy, slope);
        for x in 0..w {
            let d = x as f64 + 0.5 - ex;
            // ESF = low + contrast * CDF(main) + ringing overshoot term,
            // the latter being a signed gaussian CDF mixture.
            let edge = (norm_cdf(d, sigma) + lobe_amp * norm_cdf(d - lobe_offset, lobe_sigma))
                / (1.0 + lobe_amp);
            let v = low + (high - low) * edge + noise_sigma * rng.unit();
            data[(y * w + x) as usize] = clamp255(v);
        }
    }
    Fixture {
        key: "ringing_edge",
        title: "振铃刃边 (像素间距未知)",
        width: w,
        height: h,
        pitch_bands: vec![PitchBand {
            y0: 0,
            y1: h,
            pitch_um: None,
        }],
        note:
            "未知像素间距；LSF 为同号双高斯瓣（主瓣 σ1.1 + 偏置1.5px 的窄瓣），MTF 多次穿过 0.5。",
        image: GrayImage::new(w, h, data),
    }
}

pub fn all_fixtures() -> Vec<Fixture> {
    vec![gen_bad_rows(), gen_dual_pitch(), gen_ringing()]
}
