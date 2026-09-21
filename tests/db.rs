use edge_mtf_bench::analysis::SpectralWindow;
use edge_mtf_bench::server::memory_db;
use edge_mtf_bench::{AnalysisParams, DerivativeKernel, PitchMode, Roi, Rotation};

fn make_params(key: &str, w: u32, h: u32) -> AnalysisParams {
    AnalysisParams {
        roi: Roi {
            x0: 0,
            y0: 0,
            w,
            h,
            rotation: Rotation(0),
        },
        excluded_rows: match key {
            "bad_rows" => vec![13, 47, 48, 60],
            _ => vec![],
        },
        supersample: 4,
        derivative: DerivativeKernel::FivePoint,
        pitch_mode: PitchMode::Auto,
        manual_pitch_um: None,
        window_half_bins: 64,
        spectral_window: SpectralWindow::Hann,
    }
}

#[test]
fn create_runs_and_export_import_replay_roundtrip() {
    let db = memory_db();
    let imgs = db.list_images().unwrap();
    assert_eq!(imgs.len(), 3);
    let mut run_ids = Vec::new();
    for im in &imgs {
        let p = make_params(&im.key, im.width, im.height);
        let run = db
            .create_run(&im.key, &format!("方案-{}", im.key), &p)
            .unwrap();
        run_ids.push(run.id);
    }
    // ringing run must record 3 crossings and no lp/mm
    let runs = db.list_runs(Some("ringing_edge")).unwrap();
    let rj: serde_json::Value = serde_json::from_str(&runs[0].result_json).unwrap();
    assert_eq!(rj["crossings"].as_array().unwrap().len(), 3);
    assert!(rj["mtf50"]["f_lpmm"].is_null());

    // export, clear, import, replay verify
    let bundle = db.export_bundle().unwrap();
    db.clear_all().unwrap();
    assert_eq!(db.list_images().unwrap().len(), 0);
    assert_eq!(db.list_runs(None).unwrap().len(), 0);
    let rep = db.import_bundle(bundle).unwrap();
    assert_eq!(rep.imported_images, 3);
    assert_eq!(rep.imported_runs, 3);
    assert!(rep.errors.is_empty(), "errors: {:?}", rep.errors);
    assert!(rep.all_ok, "replay mismatch: {:?}", rep.verifies);
    for v in &rep.verifies {
        assert!(v.fingerprint_match);
        assert!(v.empty_bins_match);
        assert_eq!(v.mtf50_delta, Some(0.0));
    }
    // after re-import the same runs are readable again
    assert_eq!(db.list_runs(None).unwrap().len(), 3);
    let _ = run_ids;
}

#[test]
fn multiple_schemes_share_image_and_differ_by_fingerprint() {
    let db = memory_db();
    let im = &db.list_images().unwrap()[0];
    let mut p1 = make_params(&im.key, im.width, im.height);
    p1.supersample = 4;
    let p2 = {
        let mut q = p1.clone();
        q.supersample = 8;
        q
    };
    let p3 = {
        let mut q = p1.clone();
        q.derivative = DerivativeKernel::SevenPoint;
        q
    };
    let r1 = db.create_run(&im.key, "ss4", &p1).unwrap();
    let r2 = db.create_run(&im.key, "ss8", &p2).unwrap();
    let r3 = db.create_run(&im.key, "s7", &p3).unwrap();
    let fps: std::collections::HashSet<String> =
        [r1, r2, r3].iter().map(|r| r.fingerprint.clone()).collect();
    assert_eq!(fps.len(), 3);
    let runs = db.list_runs(Some(&im.key)).unwrap();
    assert_eq!(runs.len(), 3);
}
