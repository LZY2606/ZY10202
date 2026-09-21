use edge_mtf_bench::analysis::SpectralWindow;
use edge_mtf_bench::{
    all_fixtures, analyze, AnalysisParams, AnalysisResult, DerivativeKernel, PitchMode, Roi,
    Rotation,
};

fn base_roi(f: &edge_mtf_bench::fixtures::Fixture) -> Roi {
    Roi {
        x0: 0,
        y0: 0,
        w: f.width,
        h: f.height,
        rotation: Rotation(0),
    }
}

fn params(f: &edge_mtf_bench::fixtures::Fixture) -> AnalysisParams {
    AnalysisParams {
        roi: base_roi(f),
        excluded_rows: vec![],
        supersample: 4,
        derivative: DerivativeKernel::FivePoint,
        pitch_mode: PitchMode::Auto,
        manual_pitch_um: None,
        window_half_bins: 64,
        spectral_window: SpectralWindow::Hann,
    }
}

fn fixture(key: &str) -> edge_mtf_bench::fixtures::Fixture {
    all_fixtures().into_iter().find(|f| f.key == key).unwrap()
}

#[test]
fn bad_rows_are_detected_and_excludable() {
    let f = fixture("bad_rows");
    let r = analyze(&f.image, &f.pitch_bands, &params(&f)).unwrap();
    // four manufactured defect rows cannot host an edge -> auto excluded
    let auto = r
        .rows
        .iter()
        .filter(|x| !x.included && x.reason.contains("自动排除"))
        .count();
    assert!(
        auto >= 4,
        "expected >=4 auto-excluded defect rows, got {auto}"
    );
    for row in [13u32, 47, 48, 60] {
        let re = &r.rows[row as usize];
        assert!(!re.included, "row {row} must not be used");
    }
    // residuals are sub-pixel for a genuinely straight synthetic edge
    assert!(r.fit.rmse_px < 0.05, "rmse = {}", r.fit.rmse_px);
    assert!(r.fit.max_abs_residual_px < 0.2);

    // explicit user exclusion of additional good rows shrinks the fit set
    let mut p2 = params(&f);
    p2.excluded_rows = vec![0, 1, 2];
    let r2 = analyze(&f.image, &f.pitch_bands, &p2).unwrap();
    assert_eq!(r2.fit.n_rows, r.fit.n_rows - 3);
    assert!(r2.rows[0].reason.contains("用户排除"));
}

#[test]
fn excluded_bad_row_never_appears_in_pixels_or_bins() {
    let f = fixture("bad_rows");
    let mut p = params(&f);
    p.excluded_rows = vec![13];
    let r = analyze(&f.image, &f.pitch_bands, &p).unwrap();
    assert!(r.pixels.iter().all(|s| s.v != 13));
    assert!(!r.rows[13].included);
}

#[test]
fn empty_bins_stay_null_never_copied() {
    let f = fixture("bad_rows");
    let r = analyze(&f.image, &f.pitch_bands, &params(&f)).unwrap();
    assert!(
        r.n_empty_bins > 0,
        "slanted projection must leave empty bins"
    );
    let empties: Vec<_> = r.bins.iter().filter(|b| b.value.is_none()).collect();
    assert!(!empties.is_empty());
    for b in &empties {
        assert_eq!(b.count, 0);
    }
    // the LSF propagates null through any stencil touching an empty ESF bin
    let null_esf_indices: std::collections::HashSet<i64> =
        empties.iter().map(|b| b.index).collect();
    for l in &r.lsf {
        if null_esf_indices.contains(&l.index) {
            assert!(l.value.is_none(), "LSF bin {} must be null", l.index);
        }
    }
    // nulls never carry fake counts into the spectral core
    assert!(r.n_pixels > 0);
}

#[test]
fn bin_counts_sum_to_pixel_count() {
    let f = fixture("bad_rows");
    let r = analyze(&f.image, &f.pitch_bands, &params(&f)).unwrap();
    let counted: u32 = r.bins.iter().map(|b| b.count).sum();
    assert_eq!(counted as usize, r.n_pixels);
    // and equal the number of included pixels in the ROI
    let included_rows = r.rows.iter().filter(|x| x.included).count() as u32;
    assert_eq!(r.n_pixels as u32, included_rows * f.width);
}

#[test]
fn dual_pitch_resolution() {
    let f = fixture("dual_pitch");
    // full ROI straddles two bands -> pitch unresolved, only cycles/pixel
    let r = analyze(&f.image, &f.pitch_bands, &params(&f)).unwrap();
    assert!(r.pitch.pitch_um.is_none());
    assert!(!r.pitch.resolved);
    for m in &r.mtf {
        assert!(m.f_lpmm.is_none(), "no lp/mm allowed when pitch is unknown");
    }
    // top half only -> 3.2um, lp/mm present
    let mut p_top = params(&f);
    p_top.roi.y0 = 2;
    p_top.roi.h = 40;
    let rt = analyze(&f.image, &f.pitch_bands, &p_top).unwrap();
    assert_eq!(rt.pitch.pitch_um, Some(3.2));
    assert!(rt.mtf.iter().skip(1).take(5).all(|m| m.f_lpmm.is_some()));
    // bottom half only -> 6.4um
    let mut p_bot = params(&f);
    p_bot.roi.y0 = 52;
    p_bot.roi.h = 40;
    let rb = analyze(&f.image, &f.pitch_bands, &p_bot).unwrap();
    assert_eq!(rb.pitch.pitch_um, Some(6.4));
    // same edge in pixels -> nearly identical c/p curves, lp/mm differ by 2x
    fn pick<'a>(
        m: &'a edge_mtf_bench::AnalysisResult,
        f: f64,
    ) -> &'a edge_mtf_bench::analysis::MtfPoint {
        m.mtf
            .iter()
            .min_by(|a, b| {
                (a.f_cpp - f)
                    .abs()
                    .partial_cmp(&(b.f_cpp - f).abs())
                    .unwrap()
            })
            .unwrap()
    }
    let (at, bt) = (pick(&rt, 0.15), pick(&rb, 0.15));
    let a = at.f_lpmm.unwrap() / at.f_cpp;
    let b = bt.f_lpmm.unwrap() / bt.f_cpp;
    assert!((a - 1000.0 / 3.2).abs() < 1e-9);
    assert!((b - 1000.0 / 6.4).abs() < 1e-9);
    assert!((a / b - 2.0).abs() < 1e-9);
}

#[test]
fn unknown_pitch_never_shows_lpmm() {
    let f = fixture("ringing_edge");
    let r = analyze(&f.image, &f.pitch_bands, &params(&f)).unwrap();
    assert!(r.pitch.pitch_um.is_none());
    assert!(r.mtf.iter().all(|m| m.f_lpmm.is_none()));
    assert!(r.crossings.iter().all(|c| c.f_lpmm.is_none()));
    assert!(r.mtf50.as_ref().unwrap().f_lpmm.is_none());
}

#[test]
fn ringing_crosses_half_three_times_and_rule_picks_first_down() {
    let f = fixture("ringing_edge");
    let r = analyze(&f.image, &f.pitch_bands, &params(&f)).unwrap();
    let dirs: Vec<&str> = r.crossings.iter().map(|c| c.direction.as_str()).collect();
    assert_eq!(dirs, vec!["down", "up", "down"], "crossings = {dirs:?}");
    // the selected MTF50 is the FIRST downward crossing
    let m50 = r.mtf50.as_ref().unwrap();
    assert_eq!(m50.direction, "down");
    assert_eq!(m50.f_cpp, r.crossings[0].f_cpp);
    assert!(m50.f_cpp > 0.30 && m50.f_cpp < 0.45, "m50 = {}", m50.f_cpp);
    // lobe annotation: later crossings belong to side lobe 1
    assert_eq!(r.crossings[1].lobe, 1);
    assert_eq!(r.crossings[2].lobe, 1);
}

#[test]
fn supersample_and_kernel_change_curve_quantitatively() {
    let f = fixture("bad_rows");
    let mut p4 = params(&f);
    let r4 = analyze(&f.image, &f.pitch_bands, &p4).unwrap();
    p4.supersample = 8;
    let r8 = analyze(&f.image, &f.pitch_bands, &p4).unwrap();
    assert_ne!(r4.params_fingerprint, r8.params_fingerprint);
    assert!((r4.bin_width_px - 0.25).abs() < 1e-12);
    assert!((r8.bin_width_px - 0.125).abs() < 1e-12);
    assert!(r8.bins.len() > r4.bins.len());
    // different kernels -> different fingerprint and distinguishable numbers
    let mut pc = params(&f);
    pc.derivative = DerivativeKernel::Central3;
    let rc = analyze(&f.image, &f.pitch_bands, &pc).unwrap();
    let mut p7 = params(&f);
    p7.derivative = DerivativeKernel::SevenPoint;
    let r7 = analyze(&f.image, &f.pitch_bands, &p7).unwrap();
    assert_ne!(rc.params_fingerprint, r7.params_fingerprint);
    assert_ne!(
        format!("{:.6}", rc.mtf50.as_ref().unwrap().f_cpp),
        format!("{:.6}", r7.mtf50.as_ref().unwrap().f_cpp)
    );
}

#[test]
fn rotation_keeps_edge_consistent_and_half_pixel() {
    let f = fixture("bad_rows");
    // square ROI so the 90-rotated frame covers exactly the same pixel set
    let mut p0 = params(&f);
    p0.roi = Roi {
        x0: 10,
        y0: 8,
        w: 48,
        h: 48,
        rotation: Rotation(0),
    };
    let r0 = analyze(&f.image, &f.pitch_bands, &p0).unwrap();
    let mut p90 = p0.clone();
    p90.roi.rotation = Rotation(90);
    let r90 = analyze(&f.image, &f.pitch_bands, &p90).unwrap();
    assert_eq!(r90.pixels.len(), r0.pixels.len());
    for s in &r90.pixels {
        assert!((s.center_x.fract() - 0.5).abs() < 1e-12);
        assert!((s.center_y.fract() - 0.5).abs() < 1e-12);
    }
    // rotated pixel set is the same underlying original pixel set
    let mut a: Vec<(u32, u32)> = r0.pixels.iter().map(|s| (s.ox, s.oy)).collect();
    let mut b: Vec<(u32, u32)> = r90.pixels.iter().map(|s| (s.ox, s.oy)).collect();
    a.sort();
    b.sort();
    assert_eq!(a, b);
}

#[test]
fn fingerprint_identifies_the_scheme() {
    let f = fixture("bad_rows");
    let p = params(&f);
    let r1: AnalysisResult = analyze(&f.image, &f.pitch_bands, &p).unwrap();
    let r2 = analyze(&f.image, &f.pitch_bands, &p).unwrap();
    assert_eq!(r1.params_fingerprint, r2.params_fingerprint);
    assert!(r1.params_fingerprint.starts_with("ss4xf5xw64vhpauto-"));
}

#[test]
fn nyquist_behavior_reported() {
    let f = fixture("bad_rows");
    let r = analyze(&f.image, &f.pitch_bands, &params(&f)).unwrap();
    assert!(r.nyquist.mtf_at_nyquist >= 0.0 && r.nyquist.mtf_at_nyquist <= 1.0);
    assert!(r.nyquist.mtf_at_half_nyquist > r.nyquist.mtf_at_nyquist);
}

#[test]
fn rotated_square_roi_gives_same_mtf() {
    // A centered square ROI rotated 90 degrees samples the same physical edge;
    // MTF50 and Nyquist magnitude must agree (axis swap does not resample).
    let f = fixture("bad_rows");
    let square = Roi {
        x0: 8,
        y0: 6,
        w: 60,
        h: 60,
        rotation: Rotation(0),
    };
    let mut p0 = params(&f);
    p0.roi = square.clone();
    p0.excluded_rows = vec![];
    let r0 = analyze(&f.image, &f.pitch_bands, &p0).unwrap();
    let mut p90 = p0.clone();
    p90.roi.rotation = Rotation(90);
    let r90 = analyze(&f.image, &f.pitch_bands, &p90).unwrap();
    let m50a = r0.mtf50.as_ref().unwrap().f_cpp;
    let m50b = r90.mtf50.as_ref().unwrap().f_cpp;
    assert!((m50a - m50b).abs() < 0.01, "m50 {m50a} vs {m50b}");
    assert!((r0.nyquist.mtf_at_nyquist - r90.nyquist.mtf_at_nyquist).abs() < 0.02);
    // bin counts identical (same pixel set, permuted)
    let mut c0: Vec<u32> = r0.bins.iter().map(|b| b.count).collect();
    let mut c90: Vec<u32> = r90.bins.iter().map(|b| b.count).collect();
    c0.sort();
    c90.sort();
    assert_eq!(c0, c90);
}
