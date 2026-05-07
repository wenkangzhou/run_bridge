use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::sync::OnceLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tauri::Manager;

const DB_PATH: &str = "data/runbridge.db";
const GPX_RELATIVE_DIR: &str = "data/gpx";

// ── Writable app data dir (initialized in setup) ──────────────

static APP_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
static SYNC_PID: OnceLock<Arc<Mutex<Option<u32>>>> = OnceLock::new();

fn set_app_data_dir(dir: PathBuf) {
    APP_DATA_DIR.set(dir).ok();
}

fn app_data_dir() -> Result<PathBuf, String> {
    APP_DATA_DIR.get()
        .cloned()
        .ok_or_else(|| "App data dir not initialized".to_string())
}

fn gpx_storage_dir() -> Result<PathBuf, String> {
    let dir = app_data_dir()?.join(GPX_RELATIVE_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn python_env_vars(app_data: &Path) -> Result<Vec<(&'static str, PathBuf)>, String> {
    let gpx_dir = app_data.join("GPX_OUT");
    let tcx_dir = app_data.join("TCX_OUT");
    let fit_dir = app_data.join("FIT_OUT");
    let png_dir = app_data.join("PNG_OUT");
    let sql_file = app_data.join("run_page").join("data.db");
    let json_file = app_data.join("src").join("static").join("activities.json");
    let synced_file = app_data.join("imported.json");
    for dir in [&gpx_dir, &tcx_dir, &fit_dir, &png_dir] {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    for path in [&sql_file, &json_file, &synced_file] {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    Ok(vec![
        ("RUN_BRIDGE_GPX_FOLDER", gpx_dir),
        ("RUN_BRIDGE_TCX_FOLDER", tcx_dir),
        ("RUN_BRIDGE_FIT_FOLDER", fit_dir),
        ("RUN_BRIDGE_PNG_FOLDER", png_dir),
        ("RUN_BRIDGE_SQL_FILE", sql_file),
        ("RUN_BRIDGE_JSON_FILE", json_file),
        ("RUN_BRIDGE_SYNCED_FILE", synced_file),
    ])
}

fn resolve_gpx_path(gpx_file: &str) -> Result<PathBuf, String> {
    let path = Path::new(gpx_file);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    // Try app_data_dir first (release builds / new data)
    if let Ok(app_data) = app_data_dir() {
        let full = app_data.join(path);
        if full.exists() {
            return Ok(full);
        }
    }
    // Fallback to project_root (dev mode / legacy data)
    Ok(project_root()?.join(path))
}

fn sync_pid() -> &'static Arc<Mutex<Option<u32>>> {
    SYNC_PID.get_or_init(|| Arc::new(Mutex::new(None)))
}

#[tauri::command]
fn cancel_sync() -> Result<String, String> {
    if let Ok(guard) = sync_pid().lock() {
        if let Some(pid) = *guard {
            #[cfg(target_os = "macos")]
            { std::process::Command::new("kill").args(["-9", &pid.to_string()]).spawn().ok(); }
            #[cfg(target_os = "linux")]
            { std::process::Command::new("kill").args(["-9", &pid.to_string()]).spawn().ok(); }
            #[cfg(target_os = "windows")]
            { std::process::Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).spawn().ok(); }
            return Ok("Sync cancelled.".to_string());
        }
    }
    Ok("No sync in progress.".to_string())
}

fn project_root() -> Result<PathBuf, String> {
    // Release mode: use exe location to find bundled resources
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_dir = exe.parent().ok_or("Cannot get exe dir")?;

    // macOS .app bundle: Contents/MacOS/ -> Contents/Resources/
    let resources_dir = if exe_dir.file_name() == Some(std::ffi::OsStr::new("MacOS")) {
        exe_dir.parent().unwrap_or(exe_dir).join("Resources")
    } else {
        exe_dir.to_path_buf()
    };

    if resources_dir.join("running_page").exists() {
        return Ok(resources_dir);
    }

    // Dev mode fallback: walk up from current_dir
    let mut current = std::env::current_dir().map_err(|e| e.to_string())?;
    for _ in 0..3 {
        if current.join("running_page").exists() {
            return Ok(current);
        }
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            break;
        }
    }
    Err("Cannot find project root (running_page/ not found)".to_string())
}

// ── Data models ───────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Activity {
    pub id: i64,
    pub source: String,
    pub external_id: String,
    pub name: Option<String>,
    pub sport_type: Option<String>,
    pub start_time: Option<String>,
    pub distance_m: Option<f64>,
    pub elevation_gain_m: Option<f64>,
    pub duration_sec: Option<i64>,
    pub gpx_file: String,
}

#[derive(Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActivityFilter {
    pub source: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub distance_min: Option<f64>,
    pub distance_max: Option<f64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StravaConfig {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: Option<String>,
    pub access_token: Option<String>,
    pub expires_at: Option<i64>,
    pub athlete_id: Option<String>,
    pub authorized: bool,
}

#[derive(Debug)]
struct ActivityMeta {
    name: Option<String>,
    sport_type: Option<String>,
    start_time: Option<String>,
    distance_m: Option<f64>,
    elevation_gain_m: Option<f64>,
    duration_sec: Option<i64>,
}

// ── DB helpers ────────────────────────────────────────────────

fn db_conn() -> Result<rusqlite::Connection, String> {
    let db_path = app_data_dir()?.join(DB_PATH);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS activities (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source TEXT NOT NULL,
            external_id TEXT NOT NULL,
            name TEXT,
            sport_type TEXT,
            start_time TEXT,
            distance_m REAL,
            elevation_gain_m REAL,
            duration_sec INTEGER,
            gpx_file TEXT NOT NULL,
            is_deleted INTEGER DEFAULT 0,
            created_at TEXT,
            UNIQUE(source, external_id)
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS platforms (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            platform TEXT NOT NULL UNIQUE,
            client_id TEXT,
            client_secret TEXT,
            refresh_token TEXT,
            access_token TEXT,
            expires_at INTEGER,
            athlete_id TEXT,
            created_at TEXT,
            updated_at TEXT
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS accounts (
            platform TEXT PRIMARY KEY,
            username TEXT,
            password TEXT,
            use_token INTEGER DEFAULT 0,
            use_sid INTEGER DEFAULT 0
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    Ok(conn)
}

// ── GPX parsing ───────────────────────────────────────────────

fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_000.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    R * c
}

fn parse_gpx_file(path: &Path) -> Result<ActivityMeta, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open gpx: {}", e))?;
    let gpx_data = gpx::read(file).map_err(|e| format!("parse gpx: {}", e))?;

    let name = gpx_data.tracks.first().and_then(|t| t.name.clone());
    let sport_type = gpx_data.tracks.first().and_then(|t| t.type_.clone());

    let mut start_time: Option<String> = None;
    let mut end_time: Option<String> = None;
    let mut total_distance = 0.0;
    let mut elevation_gain = 0.0;

    for track in &gpx_data.tracks {
        for segment in &track.segments {
            let points = &segment.points;
            if points.is_empty() {
                continue;
            }

            if start_time.is_none() {
                if let Some(t) = points.first().and_then(|p| p.time) {
                    start_time = Some(t.format().map_err(|e| format!("time format: {}", e))?);
                }
            }

            for i in 1..points.len() {
                let prev = &points[i - 1];
                let curr = &points[i];

                if let (Some(pe), Some(ce)) = (prev.elevation, curr.elevation) {
                    let diff = ce - pe;
                    if diff > 0.0 {
                        elevation_gain += diff;
                    }
                }

                let (plat, plon) = (prev.point().y(), prev.point().x());
                let (clat, clon) = (curr.point().y(), curr.point().x());
                total_distance += haversine(plat, plon, clat, clon);
            }

            if let Some(t) = points.last().and_then(|p| p.time) {
                end_time = Some(t.format().map_err(|e| format!("time format: {}", e))?);
            }
        }
    }

    let duration_sec = match (&start_time, &end_time) {
        (Some(s), Some(e)) => {
            let start = chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc));
            let end = chrono::DateTime::parse_from_rfc3339(e)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc));
            match (start, end) {
                (Some(a), Some(b)) => Some((b - a).num_seconds()),
                _ => None,
            }
        }
        _ => None,
    };

    Ok(ActivityMeta {
        name,
        sport_type,
        start_time,
        distance_m: if total_distance > 0.0 { Some(total_distance) } else { None },
        elevation_gain_m: if elevation_gain > 0.0 { Some(elevation_gain) } else { None },
        duration_sec,
    })
}

// ── Import GPX from dir ───────────────────────────────────────

fn import_gpx_from_dir(source: &str, dir: &Path, skip_any_source: bool) -> Result<Vec<Activity>, String> {
    let conn = db_conn()?;
    let mut imported = Vec::new();

    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension() != Some(std::ffi::OsStr::new("gpx")) {
            continue;
        }

        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let external_id = stem.clone();

        let exists_any: bool = conn.query_row(
            "SELECT 1 FROM activities WHERE external_id = ?1 LIMIT 1",
            rusqlite::params![external_id],
            |_| Ok(true),
        ).unwrap_or(false);

        let exists_same_source: bool = conn.query_row(
            "SELECT 1 FROM activities WHERE source = ?1 AND external_id = ?2 LIMIT 1",
            rusqlite::params![source, external_id],
            |_| Ok(true),
        ).unwrap_or(false);

        if exists_same_source {
            continue;
        }

        // If exists as 'local', update source to the correct platform
        if exists_any && !skip_any_source {
            conn.execute(
                "UPDATE activities SET source = ?1 WHERE external_id = ?2 AND source = 'local'",
                rusqlite::params![source, external_id],
            ).map_err(|e| e.to_string())?;
            // fetch the updated row to return
            if let Ok(a) = conn.query_row(
                "SELECT id, source, external_id, name, sport_type, start_time,
                        distance_m, elevation_gain_m, duration_sec, gpx_file
                 FROM activities WHERE source = ?1 AND external_id = ?2 AND is_deleted = 0 LIMIT 1",
                rusqlite::params![source, external_id],
                |row| {
                    Ok(Activity {
                        id: row.get(0)?,
                        source: row.get(1)?,
                        external_id: row.get(2)?,
                        name: row.get(3)?,
                        sport_type: row.get(4)?,
                        start_time: row.get(5)?,
                        distance_m: row.get(6)?,
                        elevation_gain_m: row.get(7)?,
                        duration_sec: row.get(8)?,
                        gpx_file: row.get(9)?,
                    })
                },
            ) {
                imported.push(a);
            }
            continue;
        }

        if exists_any {
            continue;
        }

        let meta = match parse_gpx_file(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Skip dirty data: zero distance and zero elevation
        if meta.distance_m.unwrap_or(0.0) == 0.0 && meta.elevation_gain_m.unwrap_or(0.0) == 0.0 {
            continue;
        }

        // Auto-detect source from GPX track name when importing as local
        let detected_source = if source == "local" {
            if let Ok(file) = std::fs::File::open(&path) {
                if let Ok(gpx_data) = gpx::read(file) {
                    let track_name = gpx_data.tracks.first().and_then(|t| t.name.clone()).unwrap_or_default();
                    if track_name.to_lowercase().contains("codoon") {
                        "codoon"
                    } else if track_name.to_lowercase().contains("joyrun") {
                        "joyrun"
                    } else {
                        source
                    }
                } else { source }
            } else { source }
        } else { source };

        let new_name = format!("{}_{}.gpx", detected_source, external_id);
        let target_dir = gpx_storage_dir()?;
        let new_path = target_dir.join(&new_name);
        std::fs::copy(&path, &new_path).map_err(|e| e.to_string())?;

        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO activities
             (source, external_id, name, sport_type, start_time, distance_m, elevation_gain_m, duration_sec, gpx_file, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(source, external_id) DO NOTHING",
            rusqlite::params![
                detected_source,
                external_id,
                meta.name,
                meta.sport_type,
                meta.start_time,
                meta.distance_m,
                meta.elevation_gain_m,
                meta.duration_sec,
                new_path.to_string_lossy().to_string(),
                now,
            ],
        )
        .map_err(|e| e.to_string())?;

        if conn.last_insert_rowid() != 0 {
            imported.push(Activity {
                id: conn.last_insert_rowid(),
                source: detected_source.to_string(),
                external_id: stem,
                name: meta.name,
                sport_type: meta.sport_type,
                start_time: meta.start_time,
                distance_m: meta.distance_m,
                elevation_gain_m: meta.elevation_gain_m,
                duration_sec: meta.duration_sec,
                gpx_file: PathBuf::from(GPX_RELATIVE_DIR).join(&new_name).to_string_lossy().to_string(),
            });
        }
    }

    Ok(imported)
}

// ── Sync commands ─────────────────────────────────────────────

#[tauri::command]
async fn sync_codoon(
    app: tauri::AppHandle,
    mobile: String,
    password: String,
    use_token: bool,
) -> Result<String, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let env_vars = python_env_vars(&app_data)?;
    let gpx_dir = app_data.join("GPX_OUT");

    let current_dir = project_root()?;
    let script = current_dir.join("running_page/run_page/codoon_sync.py");
    if !script.exists() {
        return Err("codoon_sync.py not found".into());
    }

    let python = if cfg!(target_os = "windows") { "python" } else { "python3" };
    let work_dir = current_dir.join("running_page");

    let env_vars_clone = env_vars.clone();
    let result: Result<std::process::Output, String> = tokio::task::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(python);
        cmd.arg(&script).current_dir(&work_dir);
        for (key, value) in env_vars_clone {
            cmd.env(key, value);
        }
        if use_token {
            cmd.arg(&mobile).arg(&password).arg("--from-auth-token");
        } else {
            cmd.arg(&mobile).arg(&password);
        }
        cmd.arg("--with-gpx");
        let child = cmd.spawn().map_err(|e| e.to_string())?;
        let pid = child.id();
        *sync_pid().lock().unwrap() = Some(pid);
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        *sync_pid().lock().unwrap() = None;
        Ok(output)
    }).await.map_err(|e| e.to_string())?;

    let output = result?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(format!("Codoon sync failed (exit code: {:?})\n{}\n{}", output.status.code(), stdout, stderr));
    }

    let imported = import_gpx_from_dir("codoon", &gpx_dir, false)?;

    Ok(format!("{}\n{}\nImported {} new activities.", stdout, stderr, imported.len()))
}

#[tauri::command]
async fn sync_joyrun(
    app: tauri::AppHandle,
    phone: String,
    code: String,
    use_sid: bool,
) -> Result<String, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let env_vars = python_env_vars(&app_data)?;
    let gpx_dir = app_data.join("GPX_OUT");

    let current_dir = project_root()?;
    let script = current_dir.join("running_page/run_page/joyrun_sync.py");
    if !script.exists() {
        return Err("joyrun_sync.py not found".into());
    }

    let python = if cfg!(target_os = "windows") { "python" } else { "python3" };
    let work_dir = current_dir.join("running_page");

    let env_vars_clone = env_vars.clone();
    let result: Result<std::process::Output, String> = tokio::task::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(python);
        cmd.arg(&script).current_dir(&work_dir);
        for (key, value) in env_vars_clone {
            cmd.env(key, value);
        }
        if use_sid {
            cmd.arg(&phone).arg(&code).arg("--from-uid-sid");
        } else {
            cmd.arg(&phone).arg(&code);
        }
        cmd.arg("--with-gpx");
        let child = cmd.spawn().map_err(|e| e.to_string())?;
        let pid = child.id();
        *sync_pid().lock().unwrap() = Some(pid);
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        *sync_pid().lock().unwrap() = None;
        Ok(output)
    }).await.map_err(|e| e.to_string())?;

    let output = result?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(format!("Joyrun sync failed (exit code: {:?})\n{}\n{}", output.status.code(), stdout, stderr));
    }

    let imported = import_gpx_from_dir("joyrun", &gpx_dir, false)?;

    Ok(format!("{}\n{}\nImported {} new activities.", stdout, stderr, imported.len()))
}

#[tauri::command]
fn scan_gpx_dirs(app: tauri::AppHandle) -> Result<String, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut total = 0usize;

    let gpx_out = app_data.join("GPX_OUT");
    if gpx_out.exists() {
        total += import_gpx_from_dir("local", &gpx_out, true)?.len();
    }

    let current_dir = project_root()?;
    let data_gpx = current_dir.join("data/gpx");
    if data_gpx.exists() {
        total += import_gpx_from_dir("local", &data_gpx, true)?.len();
    }

    Ok(if total > 0 {
        format!("Scanned and imported {} new local activities.", total)
    } else {
        "No new GPX files found.".to_string()
    })
}

#[tauri::command]
fn import_local_gpx(paths: Vec<String>) -> Result<String, String> {
    let mut imported = 0usize;
    for path_str in &paths {
        let path = Path::new(path_str);
        if !path.exists() || path.extension() != Some(std::ffi::OsStr::new("gpx")) {
            continue;
        }

        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let external_id = stem;

        let conn = db_conn()?;
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM activities WHERE external_id = ?1 LIMIT 1",
                rusqlite::params![external_id],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if exists {
            continue;
        }

        let meta = parse_gpx_file(path)?;

        // Skip dirty data: zero distance and zero elevation
        if meta.distance_m.unwrap_or(0.0) == 0.0 && meta.elevation_gain_m.unwrap_or(0.0) == 0.0 {
            continue;
        }

        // Auto-detect source from GPX track name
        let detected_source = if let Ok(file) = std::fs::File::open(path) {
            if let Ok(gpx_data) = gpx::read(file) {
                let track_name = gpx_data.tracks.first().and_then(|t| t.name.clone()).unwrap_or_default();
                if track_name.to_lowercase().contains("codoon") {
                    "codoon"
                } else if track_name.to_lowercase().contains("joyrun") {
                    "joyrun"
                } else {
                    "local"
                }
            } else { "local" }
        } else { "local" };

        let new_name = format!("{}_{}.gpx", detected_source, external_id);
        let target_dir = gpx_storage_dir()?;
        let new_path = target_dir.join(&new_name);
        std::fs::copy(path, &new_path).map_err(|e| e.to_string())?;

        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO activities
             (source, external_id, name, sport_type, start_time, distance_m, elevation_gain_m, duration_sec, gpx_file, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(source, external_id) DO NOTHING",
            rusqlite::params![
                detected_source,
                external_id,
                meta.name,
                meta.sport_type,
                meta.start_time,
                meta.distance_m,
                meta.elevation_gain_m,
                meta.duration_sec,
                new_path.to_string_lossy().to_string(),
                now,
            ],
        )
        .map_err(|e| e.to_string())?;

        if conn.last_insert_rowid() != 0 {
            imported += 1;
        }
    }

    Ok(format!("Imported {} local GPX files.", imported))
}

#[tauri::command]
fn get_activities(filter: ActivityFilter) -> Result<Vec<Activity>, String> {
    let conn = db_conn()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, source, external_id, name, sport_type, start_time,
                    distance_m, elevation_gain_m, duration_sec, gpx_file
             FROM activities WHERE is_deleted = 0 ORDER BY start_time DESC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(Activity {
                id: row.get(0)?,
                source: row.get(1)?,
                external_id: row.get(2)?,
                name: row.get(3)?,
                sport_type: row.get(4)?,
                start_time: row.get(5)?,
                distance_m: row.get(6)?,
                elevation_gain_m: row.get(7)?,
                duration_sec: row.get(8)?,
                gpx_file: row.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut activities = Vec::new();
    for row in rows {
        let a = row.map_err(|e| e.to_string())?;
        if let Some(ref src) = filter.source {
            if a.source != *src {
                continue;
            }
        }
        if let Some(ref from) = filter.date_from {
            if let Some(ref st) = a.start_time {
                if st < from {
                    continue;
                }
            }
        }
        if let Some(ref to) = filter.date_to {
            if let Some(ref st) = a.start_time {
                if st > to {
                    continue;
                }
            }
        }
        if let Some(min) = filter.distance_min {
            if a.distance_m.unwrap_or(0.0) < min {
                continue;
            }
        }
        if let Some(max) = filter.distance_max {
            if a.distance_m.unwrap_or(f64::MAX) > max {
                continue;
            }
        }
        activities.push(a);
    }

    Ok(activities)
}

#[tauri::command]
fn delete_activities(ids: Vec<i64>) -> Result<String, String> {
    let conn = db_conn()?;
    for id in &ids {
        conn.execute(
            "UPDATE activities SET is_deleted = 1 WHERE id = ?1",
            rusqlite::params![id],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(format!("Deleted {} activities.", ids.len()))
}

// ── Strava OAuth ──────────────────────────────────────────────

#[tauri::command]
fn save_strava_config(client_id: String, client_secret: String) -> Result<String, String> {
    let conn = db_conn()?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO platforms (platform, client_id, client_secret, created_at, updated_at)
         VALUES ('strava', ?1, ?2, ?3, ?4)
         ON CONFLICT(platform) DO UPDATE SET
            client_id = excluded.client_id,
            client_secret = excluded.client_secret,
            updated_at = excluded.updated_at",
        rusqlite::params![client_id, client_secret, now, now],
    )
    .map_err(|e| e.to_string())?;
    Ok("Strava config saved.".to_string())
}

#[tauri::command]
fn get_strava_config() -> Result<StravaConfig, String> {
    let conn = db_conn()?;
    let row = conn.query_row(
        "SELECT client_id, client_secret, refresh_token, access_token, expires_at, athlete_id
         FROM platforms WHERE platform = 'strava' LIMIT 1",
        [],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        },
    );

    match row {
        Ok((client_id, client_secret, refresh_token, access_token, expires_at, athlete_id)) => {
            let authorized = refresh_token.is_some();
            Ok(StravaConfig {
                client_id,
                client_secret,
                refresh_token,
                access_token,
                expires_at,
                athlete_id,
                authorized,
            })
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(StravaConfig {
            client_id: String::new(),
            client_secret: String::new(),
            refresh_token: None,
            access_token: None,
            expires_at: None,
            athlete_id: None,
            authorized: false,
        }),
        Err(e) => Err(e.to_string()),
    }
}

fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", url])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn start_oauth_server(port: u16) -> Result<String, String> {
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port))
        .await
        .map_err(|e| e.to_string())?;

    let (mut socket, _) = listener
        .accept()
        .await
        .map_err(|e| e.to_string())?;

    let mut buf = [0u8; 4096];
    let n = socket
        .read(&mut buf)
        .await
        .map_err(|e| e.to_string())?;

    let request = String::from_utf8_lossy(&buf[..n]);
    let first_line = request.lines().next().ok_or("Empty request")?;
    let path = first_line.split_whitespace().nth(1).ok_or("Invalid request")?;

    let base = url::Url::parse("http://localhost").unwrap();
    let url = base.join(path).map_err(|e| e.to_string())?;
    let mut code = None;
    for (key, value) in url.query_pairs() {
        if key == "code" {
            code = Some(value.to_string());
            break;
        }
    }

    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<h1>Authorization successful! You can close this window.</h1>";
    socket
        .write_all(response.as_bytes())
        .await
        .map_err(|e| e.to_string())?;

    code.ok_or("No code found in callback".to_string())
}

#[tauri::command]
fn save_account(
    platform: String,
    username: String,
    password: String,
    use_token: bool,
    use_sid: bool,
) -> Result<(), String> {
    let conn = db_conn()?;
    conn.execute(
        "INSERT INTO accounts (platform, username, password, use_token, use_sid)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(platform) DO UPDATE SET
            username = excluded.username,
            password = excluded.password,
            use_token = excluded.use_token,
            use_sid = excluded.use_sid",
        rusqlite::params![
            platform,
            username,
            password,
            if use_token { 1 } else { 0 },
            if use_sid { 1 } else { 0 },
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub platform: String,
    pub username: String,
    pub password: String,
    pub use_token: bool,
    pub use_sid: bool,
}

#[tauri::command]
fn get_account(platform: String) -> Result<Account, String> {
    let conn = db_conn()?;
    let row = conn.query_row(
        "SELECT username, password, use_token, use_sid FROM accounts WHERE platform = ?1 LIMIT 1",
        rusqlite::params![platform],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i32>(2)?,
                row.get::<_, i32>(3)?,
            ))
        },
    );

    match row {
        Ok((username, password, use_token, use_sid)) => Ok(Account {
            platform,
            username,
            password,
            use_token: use_token != 0,
            use_sid: use_sid != 0,
        }),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err("Account not found".to_string()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
async fn authorize_strava() -> Result<String, String> {
    let config = get_strava_config()?;
    if config.client_id.is_empty() || config.client_secret.is_empty() {
        return Err("Please set Strava client_id and client_secret first.".to_string());
    }

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .port();
    drop(listener);

    let redirect_uri = format!("http://localhost:{}", port);
    let auth_url = format!(
        "https://www.strava.com/oauth/authorize?client_id={}&redirect_uri={}&response_type=code&scope=activity:write,activity:read_all",
        config.client_id,
        redirect_uri
    );

    open_browser(&auth_url)?;

    let code = start_oauth_server(port).await?;

    let client = reqwest::Client::new();
    let resp = client
        .post("https://www.strava.com/oauth/token")
        .form(&[
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("code", code.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    let refresh_token = data
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .ok_or("No refresh_token in response")?
        .to_string();
    let access_token = data
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or("No access_token in response")?
        .to_string();
    let expires_at = data.get("expires_at").and_then(|v| v.as_i64());
    let athlete_id = data
        .get("athlete")
        .and_then(|a| a.get("id"))
        .and_then(|v| v.as_i64())
        .map(|id| id.to_string());

    let conn = db_conn()?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE platforms SET refresh_token = ?1, access_token = ?2, expires_at = ?3, athlete_id = ?4, updated_at = ?5
         WHERE platform = 'strava'",
        rusqlite::params![refresh_token, access_token, expires_at, athlete_id, now],
    )
    .map_err(|e| e.to_string())?;

    Ok("Strava authorized successfully.".to_string())
}

async fn refresh_strava_token(config: &StravaConfig) -> Result<String, String> {
    let refresh_token = config
        .refresh_token
        .as_ref()
        .ok_or("No refresh token available")?;

    let client = reqwest::Client::new();
    let resp = client
        .post("https://www.strava.com/oauth/token")
        .form(&[
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    let new_access_token = data
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or("No access_token in refresh response")?
        .to_string();
    let new_refresh_token = data
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .unwrap_or(refresh_token)
        .to_string();
    let expires_at = data.get("expires_at").and_then(|v| v.as_i64());

    let conn = db_conn()?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE platforms SET access_token = ?1, refresh_token = ?2, expires_at = ?3, updated_at = ?4
         WHERE platform = 'strava'",
        rusqlite::params![new_access_token, new_refresh_token, expires_at, now],
    )
    .map_err(|e| e.to_string())?;

    Ok(new_access_token)
}

async fn get_valid_access_token() -> Result<String, String> {
    let config = get_strava_config()?;
    if !config.authorized {
        return Err("Strava not authorized. Please authorize first.".to_string());
    }

    let now = chrono::Utc::now().timestamp();
    if let Some(expires_at) = config.expires_at {
        if now >= expires_at - 300 {
            return refresh_strava_token(&config).await;
        }
    }

    config
        .access_token
        .ok_or_else(|| "No access token available".to_string())
}

#[tauri::command]
async fn upload_to_strava(ids: Vec<i64>) -> Result<Vec<String>, String> {
    let access_token = get_valid_access_token().await?;
    let conn = db_conn()?;
    let client = reqwest::Client::new();
    let mut results = Vec::new();

    for id in ids {
        let activity: Activity = conn.query_row(
            "SELECT id, source, external_id, name, sport_type, start_time,
                    distance_m, elevation_gain_m, duration_sec, gpx_file
             FROM activities WHERE id = ?1 AND is_deleted = 0 LIMIT 1",
            rusqlite::params![id],
            |row| {
                Ok(Activity {
                    id: row.get(0)?,
                    source: row.get(1)?,
                    external_id: row.get(2)?,
                    name: row.get(3)?,
                    sport_type: row.get(4)?,
                    start_time: row.get(5)?,
                    distance_m: row.get(6)?,
                    elevation_gain_m: row.get(7)?,
                    duration_sec: row.get(8)?,
                    gpx_file: row.get(9)?,
                })
            },
        ).map_err(|e| e.to_string())?;

        let gpx_path = Path::new(&activity.gpx_file);
        let file_content = std::fs::read(gpx_path)
            .map_err(|e| format!("Read GPX failed: {}", e))?;
        let file_name = Path::new(&activity.gpx_file)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let sport_type = activity.sport_type.unwrap_or_else(|| "Run".to_string());

        let mut form = reqwest::multipart::Form::new()
            .part(
                "file",
                reqwest::multipart::Part::bytes(file_content).file_name(file_name),
            )
            .text("data_type", "gpx")
            .text("sport_type", sport_type);
        // Let Strava auto-generate the title (Morning Run / Afternoon Run etc.)
        // Only pass name if we have a meaningful custom one
        if let Some(ref name) = activity.name {
            if !name.is_empty() && !name.to_lowercase().contains("gpx from") {
                form = form.text("name", name.clone());
            }
        }

        let resp = client
            .post("https://www.strava.com/api/v3/uploads")
            .bearer_auth(&access_token)
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("Upload request failed: {}", e))?;

        let status = resp.status();
        let body: serde_json::Value = resp.json().await
            .map_err(|e| format!("Parse upload response failed: {}", e))?;

        if !status.is_success() {
            let msg = body.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            results.push(format!("ID {} failed: {}", id, msg));
            continue;
        }

        let upload_id = body.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        results.push(format!("ID {} uploaded (upload_id: {})", id, upload_id));
    }

    Ok(results)
}

#[tauri::command]
fn fix_local_sources() -> Result<String, String> {
    let conn = db_conn()?;
    let mut stmt = conn
        .prepare("SELECT id, gpx_file FROM activities WHERE source = 'local' AND is_deleted = 0")
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            let id: i64 = row.get(0)?;
            let gpx_file: String = row.get(1)?;
            Ok((id, gpx_file))
        })
        .map_err(|e| e.to_string())?;

    let mut fixed = 0;
    for row in rows {
        let (id, gpx_file) = row.map_err(|e| e.to_string())?;
        let gpx_path = resolve_gpx_path(&gpx_file)?;
        let new_source = if let Ok(file) = std::fs::File::open(&gpx_path) {
            if let Ok(gpx_data) = gpx::read(file) {
                let track_name = gpx_data.tracks.first().and_then(|t| t.name.clone()).unwrap_or_default();
                if track_name.to_lowercase().contains("codoon") {
                    Some("codoon")
                } else if track_name.to_lowercase().contains("joyrun") {
                    Some("joyrun")
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        if let Some(source) = new_source {
            conn.execute(
                "UPDATE activities SET source = ?1 WHERE id = ?2",
                rusqlite::params![source, id],
            )
            .map_err(|e| e.to_string())?;
            fixed += 1;
        }
    }

    Ok(format!("Fixed {} local activities.", fixed))
}

#[tauri::command]
fn export_gpx(ids: Vec<i64>, output_dir: String) -> Result<String, String> {
    let conn = db_conn()?;
    let export_dir = PathBuf::from(&output_dir);
    std::fs::create_dir_all(&export_dir).map_err(|e| e.to_string())?;

    let mut exported = 0usize;
    for id in &ids {
        let gpx_file: String = match conn.query_row(
            "SELECT gpx_file FROM activities WHERE id = ?1 AND is_deleted = 0 LIMIT 1",
            rusqlite::params![id],
            |row| row.get(0),
        ) {
            Ok(f) => f,
            Err(_) => continue,
        };

        let src = resolve_gpx_path(&gpx_file)?;
        if !src.exists() {
            continue;
        }
        let file_name = src.file_name().unwrap_or_default();
        let dest = export_dir.join(file_name);
        std::fs::copy(src, &dest).map_err(|e| e.to_string())?;
        exported += 1;
    }

    Ok(format!("Exported {} GPX files to {}", exported, output_dir))
}

// ── App entry ─────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            set_app_data_dir(app.path().app_data_dir().map_err(|e| e.to_string())?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            sync_codoon,
            sync_joyrun,
            cancel_sync,
            scan_gpx_dirs,
            import_local_gpx,
            get_activities,
            delete_activities,
            fix_local_sources,
            save_strava_config,
            get_strava_config,
            authorize_strava,
            upload_to_strava,
            save_account,
            get_account,
            export_gpx,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
