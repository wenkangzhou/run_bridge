use std::path::{Path, PathBuf};
use std::process::Command;
use serde::{Deserialize, Serialize};

const DB_PATH: &str = "data/runbridge.db";
const GPX_DIR: &str = "data/gpx";

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
pub struct ActivityFilter {
    pub source: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub distance_min: Option<f64>,
    pub distance_max: Option<f64>,
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

fn ensure_dirs() -> Result<(), String> {
    std::fs::create_dir_all(GPX_DIR).map_err(|e| e.to_string())?;
    Ok(())
}

fn db_conn() -> Result<rusqlite::Connection, String> {
    ensure_dirs()?;
    let conn = rusqlite::Connection::open(DB_PATH).map_err(|e| e.to_string())?;
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

            // start time from first point with time
            if start_time.is_none() {
                if let Some(t) = points.first().and_then(|p| p.time) {
                    start_time = Some(t.format().map_err(|e| format!("time format: {}", e))?);
                }
            }

            for i in 1..points.len() {
                let prev = &points[i - 1];
                let curr = &points[i];

                // elevation gain
                if let (Some(pe), Some(ce)) = (prev.elevation, curr.elevation) {
                    let diff = ce - pe;
                    if diff > 0.0 {
                        elevation_gain += diff;
                    }
                }

                // distance
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
        name: None,
        sport_type,
        start_time,
        distance_m: if total_distance > 0.0 { Some(total_distance) } else { None },
        elevation_gain_m: if elevation_gain > 0.0 { Some(elevation_gain) } else { None },
        duration_sec,
    })
}

// ── Import GPX from running_page output ───────────────────────

fn import_gpx_from_dir(source: &str, dir: &Path) -> Result<Vec<Activity>, String> {
    eprintln!("[DEBUG] import_gpx_from_dir: source={}, dir={}", source, dir.display());
    let conn = db_conn()?;
    let mut imported = Vec::new();

    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    let mut count = 0;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension() != Some(std::ffi::OsStr::new("gpx")) {
            continue;
        }
        count += 1;

        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let external_id = stem.clone();
        eprintln!("[DEBUG] found gpx: {} external_id={}", path.display(), external_id);

        // skip if already imported
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM activities WHERE source = ?1 AND external_id = ?2 AND is_deleted = 0 LIMIT 1",
                rusqlite::params![source, external_id],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if exists {
            eprintln!("[DEBUG] already exists, skip");
            continue;
        }

        // parse GPX
        let meta = match parse_gpx_file(&path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[DEBUG] parse_gpx_file failed for {}: {}", path.display(), e);
                continue;
            }
        };
        eprintln!("[DEBUG] parsed: dist={:?} elev={:?} time={:?}", meta.distance_m, meta.elevation_gain_m, meta.start_time);

        // copy into data/gpx/
        let new_name = format!("{}_{}.gpx", source, external_id);
        let new_path = PathBuf::from(GPX_DIR).join(&new_name);
        std::fs::copy(&path, &new_path).map_err(|e| e.to_string())?;

        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO activities
             (source, external_id, name, sport_type, start_time, distance_m, elevation_gain_m, duration_sec, gpx_file, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(source, external_id) DO NOTHING",
            rusqlite::params![
                source,
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
            eprintln!("[DEBUG] inserted id={}", conn.last_insert_rowid());
            imported.push(Activity {
                id: conn.last_insert_rowid(),
                source: source.to_string(),
                external_id: stem,
                name: meta.name,
                sport_type: meta.sport_type,
                start_time: meta.start_time,
                distance_m: meta.distance_m,
                elevation_gain_m: meta.elevation_gain_m,
                duration_sec: meta.duration_sec,
                gpx_file: new_path.to_string_lossy().to_string(),
            });
        } else {
            eprintln!("[DEBUG] insert conflict, skipped");
        }
    }
    eprintln!("[DEBUG] total gpx files={}, imported={}", count, imported.len());

    Ok(imported)
}

// ── Tauri commands ────────────────────────────────────────────

#[tauri::command]
fn sync_codoon(
    mobile: String,
    password: String,
    use_token: bool,
) -> Result<String, String> {
    let current_dir = std::env::current_dir().map_err(|e| e.to_string())?;
    let script = current_dir.join("running_page/run_page/codoon_sync.py");
    if !script.exists() {
        return Err("codoon_sync.py not found".into());
    }

    let python = if cfg!(target_os = "windows") {
        "python"
    } else {
        "python3"
    };

    let mut cmd = Command::new(python);
    cmd.arg(&script).current_dir(current_dir.join("running_page"));

    if use_token {
        cmd.arg(&mobile).arg(&password).arg("--from-auth-token");
    } else {
        cmd.arg(&mobile).arg(&password);
    }
    cmd.arg("--with-gpx");

    let output = cmd.output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(format!("Codoon sync failed (exit code: {:?})\n{}\n{}", output.status.code(), stdout, stderr));
    }

    let gpx_dir = current_dir.join("running_page/GPX_OUT");
    let imported = import_gpx_from_dir("codoon", &gpx_dir)?;

    Ok(format!("{}\n{}\nImported {} new activities.", stdout, stderr, imported.len()))
}

#[tauri::command]
fn sync_joyrun(
    phone: String,
    code: String,
    use_sid: bool,
) -> Result<String, String> {
    let current_dir = std::env::current_dir().map_err(|e| e.to_string())?;
    let script = current_dir.join("running_page/run_page/joyrun_sync.py");
    if !script.exists() {
        return Err("joyrun_sync.py not found".into());
    }

    let python = if cfg!(target_os = "windows") {
        "python"
    } else {
        "python3"
    };

    let mut cmd = Command::new(python);
    cmd.arg(&script).current_dir(current_dir.join("running_page"));

    if use_sid {
        cmd.arg(&phone).arg(&code).arg("--from-uid-sid");
    } else {
        cmd.arg(&phone).arg(&code);
    }
    cmd.arg("--with-gpx");

    let output = cmd.output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(format!("Joyrun sync failed (exit code: {:?})\n{}\n{}", output.status.code(), stdout, stderr));
    }

    let gpx_dir = current_dir.join("running_page/GPX_OUT");
    let imported = import_gpx_from_dir("joyrun", &gpx_dir)?;

    Ok(format!("{}\n{}\nImported {} new activities.", stdout, stderr, imported.len()))
}

// ── Scan GPX dirs (GPX_OUT + data/gpx) ────────────────────────

#[tauri::command]
fn scan_gpx_dirs() -> Result<String, String> {
    let current_dir = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut total = 0usize;

    // scan running_page/GPX_OUT
    let gpx_out = current_dir.join("running_page/GPX_OUT");
    if gpx_out.exists() {
        total += import_gpx_from_dir("local", &gpx_out)?.len();
    }

    // scan data/gpx (in case files were copied there manually)
    let data_gpx = current_dir.join("data/gpx");
    if data_gpx.exists() {
        total += import_gpx_from_dir("local", &data_gpx)?.len();
    }

    Ok(if total > 0 {
        format!("Scanned and imported {} new local activities.", total)
    } else {
        "No new GPX files found.".to_string()
    })
}

// ── Import local GPX files by path ────────────────────────────

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
                "SELECT 1 FROM activities WHERE external_id = ?1 AND is_deleted = 0 LIMIT 1",
                rusqlite::params![external_id],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if exists {
            continue;
        }

        let meta = parse_gpx_file(path)?;
        let new_name = format!("local_{}.gpx", external_id);
        let new_path = PathBuf::from(GPX_DIR).join(&new_name);
        std::fs::copy(path, &new_path).map_err(|e| e.to_string())?;

        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO activities
             (source, external_id, name, sport_type, start_time, distance_m, elevation_gain_m, duration_sec, gpx_file, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(source, external_id) DO NOTHING",
            rusqlite::params![
                "local",
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
        // apply filters in Rust (dataset is small)
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

// ── App entry ─────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            sync_codoon,
            sync_joyrun,
            scan_gpx_dirs,
            import_local_gpx,
            get_activities,
            delete_activities,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
