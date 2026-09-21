use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use edge_mtf_bench::{build_router, server::memory_db};
use tower::ServiceExt;

fn app() -> axum::Router {
    build_router(Arc::new(memory_db()))
}

async fn post_json(uri: &str, json: &str) -> (StatusCode, String) {
    let resp = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(json.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024 * 1024)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn index_shows_title() {
    let resp = app()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let html = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(html.contains("刃缘解像台"));
}

#[tokio::test]
async fn create_run_and_reject_bad_params() {
    let ok = post_json(
        "/api/runs",
        r#"{"image_key":"ringing_edge","label":"t","params":{
           "roi":{"x0":0,"y0":0,"w":96,"h":80,"rotation":0},
           "supersample":4,"derivative":"five_point","pitch_mode":"auto",
           "window_half_bins":64,"spectral_window":"hann"}}"#,
    )
    .await;
    assert_eq!(ok.0, StatusCode::CREATED);
    let v: serde_json::Value = serde_json::from_str(&ok.1).unwrap();
    assert_eq!(v["result"]["crossings"].as_array().unwrap().len(), 3);
    assert!(v["result"]["mtf50"]["f_lpmm"].is_null());

    // unknown pitch must never surface lp/mm anywhere in the MTF array
    let any_lpmm = v["result"]["mtf"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| !p["f_lpmm"].is_null());
    assert!(!any_lpmm);

    let bad = post_json(
        "/api/runs",
        r#"{"image_key":"ringing_edge","label":"t","params":{
           "roi":{"x0":0,"y0":0,"w":9999,"h":80,"rotation":0},
           "supersample":4,"derivative":"five_point","pitch_mode":"auto",
           "window_half_bins":64}}"#,
    )
    .await;
    assert_eq!(bad.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn clear_then_import_replay_via_http() {
    let application = Arc::new(memory_db());
    let router = build_router(application.clone());
    // create one run
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/runs")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"image_key":"bad_rows","label":"t","params":{
                       "roi":{"x0":0,"y0":0,"w":96,"h":72,"rotation":0},
                       "excluded_rows":[13,47,48,60],
                       "supersample":8,"derivative":"seven_point","pitch_mode":"auto",
                       "window_half_bins":64,"spectral_window":"hann"}}"#
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let export = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/export")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(export.into_body(), 64 * 1024 * 1024)
        .await
        .unwrap();

    let _ = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/clear")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let imp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/import")
                .header("content-type", "application/json")
                .body(Body::from(bytes.to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(imp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(imp.into_body(), 64 * 1024 * 1024)
        .await
        .unwrap();
    let rep: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(rep["all_ok"], true);
}
