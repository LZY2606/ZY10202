//! SQLite persistence: images, analysis runs (full evidence JSON) and a run
//! log. Export produces one self-contained JSON document; import clears the
//! tables, restores the rows and RE-ANALYZES every run from stored pixels +
//! parameters to verify the numbers replay identically.

use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::analysis::{analyze, AnalysisParams, AnalysisResult};
use crate::fixtures::Fixture;
use crate::image::{checksum_hex, GrayImage, PitchBand};

pub struct Db {
    pub conn: Mutex<Connection>,
}

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageRow {
    pub key: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub pixel_checksum: String,
    pub pitch_bands_json: String,
    pub note: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRow {
    pub id: i64,
    pub image_key: String,
    pub label: String,
    pub fingerprint: String,
    pub params_json: String,
    pub result_json: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogRow {
    pub id: i64,
    pub created_at: String,
    pub action: String,
    pub detail: String,
}

#[derive(Serialize, Deserialize)]
pub struct ExportBundle {
    pub app: String,
    pub schema_version: u32,
    pub exported_at: String,
    pub images: Vec<ImageRow>,
    pub runs: Vec<RunRow>,
    pub run_log: Vec<LogRow>,
}

#[derive(Serialize, Debug)]
pub struct VerifyItem {
    pub run_id: i64,
    pub label: String,
    pub fingerprint_match: bool,
    pub mtf50_cpp_stored: Option<f64>,
    pub mtf50_cpp_replayed: Option<f64>,
    pub mtf50_delta: Option<f64>,
    pub n_bins_match: bool,
    pub empty_bins_match: bool,
    pub ok: bool,
}

fn now_ts() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // deterministic-ish UTC stamp without a chrono dependency
    format_unix(secs)
}

fn format_unix(secs: u64) -> String {
    let days = secs / 86400;
    let rem = secs % 86400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // civil date from days since 1970 (Howard Hinnant's algorithm)
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if m <= 2 { y + 1 } else { y };
    format!("{yr:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

impl Db {
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn log(&self, action: &str, detail: &str) {
        if let Ok(c) = self.conn.lock() {
            let _ = c.execute(
                "INSERT INTO run_log (created_at, action, detail) VALUES (?1, ?2, ?3)",
                params![now_ts(), action, detail],
            );
        }
    }

    pub fn seed_fixtures(&self, fixtures: &[Fixture]) -> rusqlite::Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let existing: i64 = tx.query_row("SELECT COUNT(*) FROM images", [], |r| r.get(0))?;
        if existing == 0 {
            for f in fixtures {
                let bands: Vec<PitchBand> = f.pitch_bands.to_vec();
                let bands_json = serde_json::to_string(&bands).unwrap();
                let checksum = checksum_hex(&f.image.data);
                tx.execute(
                    "INSERT INTO images (key, title, width, height, pixel_checksum, pitch_bands_json, note, data)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                    params![f.key, f.title, f.width, f.height, checksum, bands_json, f.note, f.image.data],
                )?;
            }
        }
        tx.commit()
    }
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS images (
    key              TEXT PRIMARY KEY,
    title            TEXT NOT NULL,
    width            INTEGER NOT NULL,
    height           INTEGER NOT NULL,
    pixel_checksum   TEXT NOT NULL,
    pitch_bands_json TEXT NOT NULL,
    note             TEXT NOT NULL,
    data             BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS runs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    image_key   TEXT NOT NULL REFERENCES images(key),
    label       TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    params_json TEXT NOT NULL,
    result_json TEXT NOT NULL,
    created_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_runs_image ON runs(image_key);
CREATE TABLE IF NOT EXISTS run_log (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL,
    action     TEXT NOT NULL,
    detail     TEXT NOT NULL
);
"#;

#[derive(serde::Serialize)]
pub struct ImageMeta {
    pub key: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub pixel_checksum: String,
    pub pitch_bands: Vec<PitchBand>,
    pub note: String,
}

impl Db {
    pub fn list_images(&self) -> rusqlite::Result<Vec<ImageMeta>> {
        let c = self.conn.lock().unwrap();
        let mut stmt = c.prepare(
            "SELECT key,title,width,height,pixel_checksum,pitch_bands_json,note FROM images ORDER BY rowid",
        )?;
        let rows = stmt.query_map([], |r| {
            let bands_json: String = r.get(5)?;
            Ok(ImageMeta {
                key: r.get(0)?,
                title: r.get(1)?,
                width: r.get(2)?,
                height: r.get(3)?,
                pixel_checksum: r.get(4)?,
                pitch_bands: serde_json::from_str(&bands_json).unwrap_or_default(),
                note: r.get(6)?,
            })
        })?;
        rows.collect()
    }

    pub fn get_image(&self, key: &str) -> rusqlite::Result<Option<ImageRow>> {
        let c = self.conn.lock().unwrap();
        let mut stmt = c.prepare(
            "SELECT key,title,width,height,pixel_checksum,pitch_bands_json,note,data FROM images WHERE key=?1",
        )?;
        let mut q = stmt.query(params![key])?;
        match q.next()? {
            Some(r) => Ok(Some(ImageRow {
                key: r.get(0)?,
                title: r.get(1)?,
                width: r.get(2)?,
                height: r.get(3)?,
                pixel_checksum: r.get(4)?,
                pitch_bands_json: r.get(5)?,
                note: r.get(6)?,
                data: r.get(7)?,
            })),
            None => Ok(None),
        }
    }

    pub fn list_runs(&self, image_key: Option<&str>) -> rusqlite::Result<Vec<RunRow>> {
        let c = self.conn.lock().unwrap();
        let mut stmt = if image_key.is_some() {
            c.prepare(
                "SELECT id,image_key,label,fingerprint,params_json,result_json,created_at
                 FROM runs WHERE image_key=?1 ORDER BY id",
            )?
        } else {
            c.prepare(
                "SELECT id,image_key,label,fingerprint,params_json,result_json,created_at
                 FROM runs ORDER BY id",
            )?
        };
        let mapper = |r: &rusqlite::Row| {
            Ok(RunRow {
                id: r.get(0)?,
                image_key: r.get(1)?,
                label: r.get(2)?,
                fingerprint: r.get(3)?,
                params_json: r.get(4)?,
                result_json: r.get(5)?,
                created_at: r.get(6)?,
            })
        };
        let rows = if let Some(k) = image_key {
            stmt.query_map(params![k], mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map([], mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(rows)
    }

    pub fn list_logs(&self) -> rusqlite::Result<Vec<LogRow>> {
        let c = self.conn.lock().unwrap();
        let mut stmt = c.prepare(
            "SELECT id,created_at,action,detail FROM run_log ORDER BY id DESC LIMIT 200",
        )?;
        let mapped = stmt.query_map([], |r| {
            Ok(LogRow {
                id: r.get(0)?,
                created_at: r.get(1)?,
                action: r.get(2)?,
                detail: r.get(3)?,
            })
        })?;
        let out: rusqlite::Result<Vec<LogRow>> = mapped.collect();
        out
    }

    /// Run an analysis from stored image pixels and persist the full evidence.
    pub fn create_run(
        &self,
        image_key: &str,
        label: &str,
        params: &AnalysisParams,
    ) -> Result<RunRow, String> {
        let img_row = self
            .get_image(image_key)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("image {image_key} not found"))?;
        let checksum = checksum_hex(&img_row.data);
        if checksum != img_row.pixel_checksum {
            return Err(format!(
                "像素校验和不一致：存储 {}，重算 {}",
                img_row.pixel_checksum, checksum
            ));
        }
        let img = GrayImage::new(img_row.width, img_row.height, img_row.data.clone());
        let bands: Vec<PitchBand> =
            serde_json::from_str(&img_row.pitch_bands_json).map_err(|e| e.to_string())?;
        let result: AnalysisResult = analyze(&img, &bands, params)?;
        let result_json = serde_json::to_string(&result).map_err(|e| e.to_string())?;
        let params_json = serde_json::to_string(params).map_err(|e| e.to_string())?;
        let ts = now_ts();
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO runs (image_key,label,fingerprint,params_json,result_json,created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                image_key,
                label,
                result.params_fingerprint,
                params_json,
                result_json,
                ts
            ],
        )
        .map_err(|e| e.to_string())?;
        let id = c.last_insert_rowid();
        Ok(RunRow {
            id,
            image_key: image_key.to_string(),
            label: label.to_string(),
            fingerprint: result.params_fingerprint,
            params_json,
            result_json,
            created_at: ts,
        })
    }

    pub fn delete_run(&self, id: i64) -> rusqlite::Result<usize> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM runs WHERE id=?1", params![id])
    }

    pub fn clear_all(&self) -> rusqlite::Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute_batch("DELETE FROM runs; DELETE FROM images; DELETE FROM run_log;")?;
        tx.commit()
    }

    pub fn export_bundle(&self) -> rusqlite::Result<ExportBundle> {
        let c = self.conn.lock().unwrap();
        let images = {
            let mut stmt = c.prepare(
                "SELECT key,title,width,height,pixel_checksum,pitch_bands_json,note,data FROM images ORDER BY rowid",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(ImageRow {
                    key: r.get(0)?,
                    title: r.get(1)?,
                    width: r.get(2)?,
                    height: r.get(3)?,
                    pixel_checksum: r.get(4)?,
                    pitch_bands_json: r.get(5)?,
                    note: r.get(6)?,
                    data: r.get(7)?,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let runs = {
            let mut stmt = c.prepare(
                "SELECT id,image_key,label,fingerprint,params_json,result_json,created_at FROM runs ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(RunRow {
                    id: r.get(0)?,
                    image_key: r.get(1)?,
                    label: r.get(2)?,
                    fingerprint: r.get(3)?,
                    params_json: r.get(4)?,
                    result_json: r.get(5)?,
                    created_at: r.get(6)?,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let run_log = {
            let mut stmt =
                c.prepare("SELECT id,created_at,action,detail FROM run_log ORDER BY id")?;
            let rows = stmt.query_map([], |r| {
                Ok(LogRow {
                    id: r.get(0)?,
                    created_at: r.get(1)?,
                    action: r.get(2)?,
                    detail: r.get(3)?,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(ExportBundle {
            app: "edge_mtf_bench".into(),
            schema_version: SCHEMA_VERSION,
            exported_at: now_ts(),
            images,
            runs,
            run_log,
        })
    }
}

#[derive(serde::Serialize, Debug)]
pub struct ImportReport {
    pub imported_images: usize,
    pub imported_runs: usize,
    pub verifies: Vec<VerifyItem>,
    pub all_ok: bool,
    pub errors: Vec<String>,
}

fn r64(v: f64) -> f64 {
    if v.is_finite() {
        (v * 1e10).round() / 1e10
    } else {
        v
    }
}

impl Db {
    /// Clear the database, restore the bundle, then replay every analysis from
    /// the imported pixel data + parameters and compare key quantities.
    pub fn import_bundle(&self, bundle: ExportBundle) -> rusqlite::Result<ImportReport> {
        {
            let mut c = self.conn.lock().unwrap();
            let tx = c.transaction()?;
            tx.execute_batch("DELETE FROM runs; DELETE FROM images; DELETE FROM run_log;")?;
            for im in &bundle.images {
                tx.execute(
                    "INSERT INTO images (key,title,width,height,pixel_checksum,pitch_bands_json,note,data)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                    params![im.key, im.title, im.width, im.height, im.pixel_checksum,
                            im.pitch_bands_json, im.note, im.data],
                )?;
            }
            for rn in &bundle.runs {
                tx.execute(
                    "INSERT INTO runs (id,image_key,label,fingerprint,params_json,result_json,created_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![rn.id, rn.image_key, rn.label, rn.fingerprint,
                            rn.params_json, rn.result_json, rn.created_at],
                )?;
            }
            for lg in &bundle.run_log {
                tx.execute(
                    "INSERT INTO run_log (id,created_at,action,detail) VALUES (?1,?2,?3,?4)",
                    params![lg.id, lg.created_at, lg.action, lg.detail],
                )?;
            }
            tx.commit()?;
        }
        self.log(
            "import",
            &format!(
                "导入 {} 图像 / {} 运行记录并开始重放复核",
                bundle.images.len(),
                bundle.runs.len()
            ),
        );

        let mut verifies = Vec::new();
        let mut errors = Vec::new();
        for rn in &bundle.runs {
            let stored: AnalysisResult = match serde_json::from_str(&rn.result_json) {
                Ok(v) => v,
                Err(e) => {
                    errors.push(format!("run {}: result_json 解析失败: {e}", rn.id));
                    continue;
                }
            };
            let params: AnalysisParams = match serde_json::from_str(&rn.params_json) {
                Ok(v) => v,
                Err(e) => {
                    errors.push(format!("run {}: params_json 解析失败: {e}", rn.id));
                    continue;
                }
            };
            let s50 = stored.mtf50.as_ref().map(|c| c.f_cpp);
            let verify = match self.get_image(&rn.image_key)? {
                Some(im) => {
                    if checksum_hex(&im.data) != im.pixel_checksum {
                        VerifyItem {
                            run_id: rn.id,
                            label: rn.label.clone(),
                            fingerprint_match: false,
                            mtf50_cpp_stored: stored.mtf50.as_ref().map(|c| c.f_cpp),
                            mtf50_cpp_replayed: None,
                            mtf50_delta: None,
                            n_bins_match: false,
                            empty_bins_match: false,
                            ok: false,
                        }
                    } else {
                        let img = GrayImage::new(im.width, im.height, im.data);
                        let bands: Vec<PitchBand> =
                            serde_json::from_str(&im.pitch_bands_json).unwrap_or_default();
                        match analyze(&img, &bands, &params) {
                            Ok(re) => {
                                let r50 = re.mtf50.as_ref().map(|c| c.f_cpp);
                                let delta = match (s50, r50) {
                                    (Some(a), Some(b)) => Some(r64((a - b).abs())),
                                    (None, None) => Some(0.0),
                                    _ => None,
                                };
                                let fp_match = re.params_fingerprint == rn.fingerprint
                                    && re.params_fingerprint == stored.params_fingerprint;
                                let bins_ok = re.bins.len() == stored.bins.len();
                                let empty_ok = re.n_empty_bins == stored.n_empty_bins
                                    && re.bins.iter().zip(&stored.bins).all(|(a, b)| {
                                        a.value.is_none() == b.value.is_none() && a.count == b.count
                                    });
                                let val_ok = match delta {
                                    Some(d) => d <= 1e-8,
                                    None => false,
                                };
                                VerifyItem {
                                    run_id: rn.id,
                                    label: rn.label.clone(),
                                    fingerprint_match: fp_match,
                                    mtf50_cpp_stored: s50,
                                    mtf50_cpp_replayed: r50,
                                    mtf50_delta: delta,
                                    n_bins_match: bins_ok,
                                    empty_bins_match: empty_ok,
                                    ok: fp_match && bins_ok && empty_ok && val_ok,
                                }
                            }
                            Err(e) => {
                                errors.push(format!("run {}: 重放失败: {e}", rn.id));
                                VerifyItem {
                                    run_id: rn.id,
                                    label: rn.label.clone(),
                                    fingerprint_match: false,
                                    mtf50_cpp_stored: s50,
                                    mtf50_cpp_replayed: None,
                                    mtf50_delta: None,
                                    n_bins_match: false,
                                    empty_bins_match: false,
                                    ok: false,
                                }
                            }
                        }
                    }
                }
                None => {
                    errors.push(format!("run {}: 缺少图像 {}", rn.id, rn.image_key));
                    continue;
                }
            };
            verifies.push(verify);
        }
        let all_ok = errors.is_empty() && verifies.iter().all(|v| v.ok);
        self.log(
            "verify",
            if all_ok {
                "重放复核全部通过"
            } else {
                "重放复核存在不一致项，请查看导入报告"
            },
        );
        Ok(ImportReport {
            imported_images: bundle.images.len(),
            imported_runs: bundle.runs.len(),
            verifies,
            all_ok,
            errors,
        })
    }
}
