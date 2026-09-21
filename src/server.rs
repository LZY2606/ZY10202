use std::sync::Arc;

use axum::{
    body::Body,
    extract::DefaultBodyLimit,
    extract::{Path, Query, State},
    http::HeaderValue,
    http::{header, StatusCode},
    response::Response as AxResponse,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use tower_http::services::ServeDir;

use crate::analysis::AnalysisParams;
use crate::db::{Db, ExportBundle};
use crate::fixtures::all_fixtures;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
}

pub fn build_router(db: Arc<Db>) -> Router {
    let api = Router::new()
        .route("/images", get(list_images))
        .route("/images/{key}/pixels", get(image_pixels))
        .route("/runs", get(list_runs).post(create_run))
        .route("/runs/{id}", get(get_run).delete(delete_run))
        .route("/logs", get(list_logs))
        .route("/export", get(export_bundle))
        .route("/import", post(import_bundle))
        .route("/reseed", post(reseed))
        .route("/clear", post(clear_db));

    let static_svc = ServeDir::new("static").append_index_html_on_directories(true);
    Router::new()
        .nest("/api", api)
        .fallback_service(static_svc)
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        .layer(axum::middleware::map_response(no_cache))
        .with_state(AppState { db })
}

async fn no_cache(mut resp: AxResponse) -> AxResponse {
    resp.headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    resp
}

async fn list_images(State(st): State<AppState>) -> Response {
    match st.db.list_images() {
        Ok(v) => Json(v).into_response(),
        Err(e) => err500(e),
    }
}

async fn image_pixels(State(st): State<AppState>, Path(key): Path<String>) -> Response {
    match st.db.get_image(&key) {
        Ok(Some(im)) => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/octet-stream")
            .header("X-Image-Width", im.width.to_string())
            .header("X-Image-Height", im.height.to_string())
            .header("X-Checksum", im.pixel_checksum)
            .body(Body::from(im.data))
            .unwrap(),
        Ok(None) => (StatusCode::NOT_FOUND, "image not found").into_response(),
        Err(e) => err500(e),
    }
}

#[derive(Deserialize)]
struct RunsQuery {
    image_key: Option<String>,
}

async fn list_runs(State(st): State<AppState>, q: Query<RunsQuery>) -> Response {
    match st.db.list_runs(q.image_key.as_deref()) {
        Ok(v) => {
            let slim: Vec<_> = v
                .into_iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.id,
                        "image_key": r.image_key,
                        "label": r.label,
                        "fingerprint": r.fingerprint,
                        "created_at": r.created_at,
                    })
                })
                .collect();
            Json(slim).into_response()
        }
        Err(e) => err500(e),
    }
}

#[derive(Deserialize)]
struct CreateRunReq {
    image_key: String,
    label: String,
    params: AnalysisParams,
}

async fn create_run(State(st): State<AppState>, Json(req): Json<CreateRunReq>) -> Response {
    let label = if req.label.trim().is_empty() {
        "未命名方案".to_string()
    } else {
        req.label.trim().to_string()
    };
    match st.db.create_run(&req.image_key, &label, &req.params) {
        Ok(run) => {
            st.db.log(
                "create_run",
                &format!(
                    "图像 {} 方案 #{} ({}, {})",
                    req.image_key, run.id, label, run.fingerprint
                ),
            );
            (
                StatusCode::CREATED,
                Json(serde_json::json!({
                    "id": run.id,
                    "fingerprint": run.fingerprint,
                    "result": serde_json::from_str::<serde_json::Value>(&run.result_json).unwrap(),
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

async fn get_run(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    match st.db.list_runs(None) {
        Ok(runs) => match runs.into_iter().find(|r| r.id == id) {
            Some(r) => {
                let mut v = serde_json::to_value(&r).unwrap();
                v["result"] = serde_json::from_str(&r.result_json).unwrap();
                v["params"] = serde_json::from_str(&r.params_json).unwrap();
                if let serde_json::Value::Object(m) = &mut v {
                    m.remove("result_json");
                    m.remove("params_json");
                }
                Json(v).into_response()
            }
            None => (StatusCode::NOT_FOUND, "run not found").into_response(),
        },
        Err(e) => err500(e),
    }
}

async fn delete_run(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    match st.db.delete_run(id) {
        Ok(n) => {
            if n == 0 {
                (StatusCode::NOT_FOUND, "run not found").into_response()
            } else {
                st.db.log("delete_run", &format!("删除方案 #{id}"));
                StatusCode::NO_CONTENT.into_response()
            }
        }
        Err(e) => err500(e),
    }
}

async fn list_logs(State(st): State<AppState>) -> Response {
    match st.db.list_logs() {
        Ok(v) => Json(v).into_response(),
        Err(e) => err500(e),
    }
}

async fn export_bundle(State(st): State<AppState>) -> Response {
    match st.db.export_bundle() {
        Ok(b) => match serde_json::to_vec(&b) {
            Ok(bytes) => Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"edge_bench_export.json\"",
                )
                .body(Body::from(bytes))
                .unwrap(),
            Err(e) => err500(e),
        },
        Err(e) => err500(e),
    }
}

async fn import_bundle(State(st): State<AppState>, body: axum::body::Bytes) -> Response {
    let bundle: ExportBundle = match serde_json::from_slice(&body) {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("导入文件解析失败: {e}")})),
            )
                .into_response()
        }
    };
    if bundle.app != "edge_mtf_bench" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "不是刃缘解像台导出的文件"})),
        )
            .into_response();
    }
    match st.db.import_bundle(bundle) {
        Ok(rep) => Json(rep).into_response(),
        Err(e) => err500(e),
    }
}

async fn clear_db(State(st): State<AppState>) -> Response {
    match st.db.clear_all() {
        Ok(()) => {
            st.db.log("clear", "数据库已清空（图像/方案/日志）");
            Json(serde_json::json!({"ok": true})).into_response()
        }
        Err(e) => err500(e),
    }
}

async fn reseed(State(st): State<AppState>) -> Response {
    match st.db.seed_fixtures(&all_fixtures()) {
        Ok(()) => {
            st.db.log("reseed", "重新植入三个固定 fixture");
            Json(serde_json::json!({"ok": true})).into_response()
        }
        Err(e) => err500(e),
    }
}

fn err500<E: std::fmt::Display>(e: E) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": e.to_string()})),
    )
        .into_response()
}

/// Convenience used in tests: build an in-memory database seeded with fixtures.
#[allow(dead_code)]
pub fn memory_db() -> crate::db::Db {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE images (key TEXT PRIMARY KEY,title TEXT NOT NULL,width INTEGER NOT NULL,
         height INTEGER NOT NULL,pixel_checksum TEXT NOT NULL,pitch_bands_json TEXT NOT NULL,
         note TEXT NOT NULL,data BLOB NOT NULL);
         CREATE TABLE runs (id INTEGER PRIMARY KEY AUTOINCREMENT,image_key TEXT NOT NULL,
         label TEXT NOT NULL,fingerprint TEXT NOT NULL,params_json TEXT NOT NULL,
         result_json TEXT NOT NULL,created_at TEXT NOT NULL);
         CREATE TABLE run_log (id INTEGER PRIMARY KEY AUTOINCREMENT,created_at TEXT NOT NULL,
         action TEXT NOT NULL,detail TEXT NOT NULL);",
    )
    .unwrap();
    let db = crate::db::Db {
        conn: std::sync::Mutex::new(conn),
    };
    db.seed_fixtures(&all_fixtures()).unwrap();
    db
}
