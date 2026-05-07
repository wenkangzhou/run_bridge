import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { open } from "@tauri-apps/plugin-dialog";
import { check } from "@tauri-apps/plugin-updater";
import "./App.css";

type Activity = {
  id: number;
  source: string;
  external_id: string;
  name: string | null;
  sport_type: string | null;
  start_time: string | null;
  distance_m: number | null;
  elevation_gain_m: number | null;
  duration_sec: number | null;
  gpx_file: string;
};

type StravaConfig = {
  client_id: string;
  client_secret: string;
  refresh_token: string | null;
  access_token: string | null;
  expires_at: number | null;
  athlete_id: string | null;
  authorized: boolean;
};

type View = "sync" | "activities" | "platforms";

function fmtDist(m: number | null): string {
  if (m == null) return "-";
  if (m >= 1000) return `${(m / 1000).toFixed(2)} km`;
  return `${m.toFixed(0)} m`;
}

function fmtDur(sec: number | null): string {
  if (sec == null) return "-";
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

function fmtDate(iso: string | null): string {
  if (!iso) return "-";
  const d = new Date(iso);
  return d.toLocaleString("zh-CN", { year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

// ── Sync Page ─────────────────────────────────────────────────

type Account = {
  platform: string;
  username: string;
  password: string;
  use_token: boolean;
  use_sid: boolean;
};

function SyncPage() {
  const [platform, setPlatform] = useState<"codoon" | "joyrun">("codoon");
  const [log, setLog] = useState("");
  const [loading, setLoading] = useState(false);

  const [cdMobile, setCdMobile] = useState("");
  const [cdPassword, setCdPassword] = useState("");
  const [cdUseToken, setCdUseToken] = useState(false);

  const [jrPhone, setJrPhone] = useState("");
  const [jrCode, setJrCode] = useState("");
  const [jrUseSid, setJrUseSid] = useState(false);

  useEffect(() => {
    loadAccount("codoon");
    loadAccount("joyrun");
  }, []);

  async function loadAccount(p: string) {
    try {
      const acc = await invoke<Account>("get_account", { platform: p });
      if (p === "codoon") {
        setCdMobile(acc.username);
        setCdPassword(acc.password);
        setCdUseToken(acc.use_token ?? false);
      } else {
        setJrPhone(acc.username);
        setJrCode(acc.password);
        setJrUseSid(acc.use_sid ?? false);
      }
    } catch {
      // account not found, ignore
    }
  }

  async function handleSync() {
    // save account immediately so credentials are remembered even if sync fails
    try {
      if (platform === "codoon") {
        await invoke("save_account", { platform: "codoon", username: cdMobile, password: cdPassword, useToken: cdUseToken ?? false, useSid: false });
      } else {
        await invoke("save_account", { platform: "joyrun", username: jrPhone, password: jrCode, useToken: false, useSid: jrUseSid ?? false });
      }
    } catch (e) {
      console.error("Failed to save account:", e);
    }

    setLoading(true);
    setLog("Starting sync...\n");
    try {
      const args =
        platform === "codoon"
          ? { mobile: cdMobile, password: cdPassword, useToken: cdUseToken ?? false }
          : { phone: jrPhone, code: jrCode, useSid: jrUseSid ?? false };
      console.log("invoke args:", platform, args);
      const result = await invoke(platform === "codoon" ? "sync_codoon" : "sync_joyrun", args);
      setLog(String(result));
    } catch (err) {
      setLog(String(err));
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="sync-page">
      <div className="platform-tabs">
        <button className={platform === "codoon" ? "active" : ""} onClick={() => setPlatform("codoon")}>
          咕咚 (Codoon)
        </button>
        <button className={platform === "joyrun" ? "active" : ""} onClick={() => setPlatform("joyrun")}>
          悦跑圈 (Joyrun)
        </button>
      </div>

      {platform === "codoon" && (
        <>
          <div className="form-group">
            <label>手机号 / Refresh Token</label>
            <input type="text" value={cdMobile} onChange={(e) => setCdMobile(e.currentTarget.value)} placeholder={cdUseToken ? "refresh_token" : "mobile number"} />
          </div>
          <div className="form-group">
            <label>密码 / User ID</label>
            <input type="password" value={cdPassword} onChange={(e) => setCdPassword(e.currentTarget.value)} placeholder={cdUseToken ? "user_id" : "password"} />
          </div>
          <div className="form-group checkbox">
            <label>
              <input type="checkbox" checked={cdUseToken} onChange={(e) => setCdUseToken(e.currentTarget.checked)} />
              使用 Refresh Token 登录
            </label>
          </div>
        </>
      )}

      {platform === "joyrun" && (
        <>
          <div className="form-group">
            <label>手机号 / UID</label>
            <input type="text" value={jrPhone} onChange={(e) => setJrPhone(e.currentTarget.value)} placeholder={jrUseSid ? "uid" : "phone number"} />
          </div>
          <div className="form-group">
            <label>验证码 / SID</label>
            <input type="text" value={jrCode} onChange={(e) => setJrCode(e.currentTarget.value)} placeholder={jrUseSid ? "sid" : "SMS code"} />
          </div>
          <div className="form-group checkbox">
            <label>
              <input type="checkbox" checked={jrUseSid} onChange={(e) => setJrUseSid(e.currentTarget.checked)} />
              使用 UID + SID 登录
            </label>
          </div>
        </>
      )}

      <div className="sync-actions">
        <button onClick={handleSync} disabled={loading} className="sync-btn">
          {loading ? "Syncing..." : "Start Sync"}
        </button>
        {loading && (
          <button onClick={async () => { try { const r = await invoke("cancel_sync"); setLog(String(r)); setLoading(false); } catch (e) { setLog(String(e)); } }} className="cancel-btn">
            Cancel
          </button>
        )}
      </div>
      {log && <div className="log-box"><pre>{log}</pre></div>}
    </div>
  );
}

// ── Platforms Page ────────────────────────────────────────────

function PlatformsPage() {
  const [config, setConfig] = useState<StravaConfig>({
    client_id: "",
    client_secret: "",
    refresh_token: null,
    access_token: null,
    expires_at: null,
    athlete_id: null,
    authorized: false,
  });
  const [saving, setSaving] = useState(false);
  const [authorizing, setAuthorizing] = useState(false);
  const [msg, setMsg] = useState("");

  async function loadConfig() {
    try {
      const c = await invoke<StravaConfig>("get_strava_config");
      setConfig(c);
    } catch (err) {
      console.error(err);
    }
  }

  useEffect(() => {
    loadConfig();
  }, []);

  async function handleSave() {
    setSaving(true);
    setMsg("");
    try {
      await invoke("save_strava_config", {
        clientId: config.client_id,
        clientSecret: config.client_secret,
      });
      setMsg("Config saved.");
      await loadConfig();
    } catch (err) {
      setMsg(String(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleAuthorize() {
    setAuthorizing(true);
    setMsg("Opening browser for Strava authorization...");
    try {
      const result = await invoke<string>("authorize_strava");
      setMsg(result);
      await loadConfig();
    } catch (err) {
      setMsg(String(err));
    } finally {
      setAuthorizing(false);
    }
  }

  return (
    <div className="platforms-page">
      <h2>Platforms</h2>
      <div className="platform-card">
        <div className="platform-header">
          <span className="platform-name">Strava</span>
          <span className={`platform-status ${config.authorized ? "ok" : "warn"}`}>
            {config.authorized ? "已授权" : "未授权"}
          </span>
        </div>

        {config.athlete_id && (
          <p className="platform-detail">Athlete ID: {config.athlete_id}</p>
        )}

        <div className="form-group">
          <label>Client ID</label>
          <input
            type="text"
            value={config.client_id}
            onChange={(e) => setConfig({ ...config, client_id: e.currentTarget.value })}
          />
        </div>
        <div className="form-group">
          <label>Client Secret</label>
          <input
            type="password"
            value={config.client_secret}
            onChange={(e) => setConfig({ ...config, client_secret: e.currentTarget.value })}
          />
        </div>

        <div className="platform-actions">
          <button onClick={handleSave} disabled={saving}>
            {saving ? "Saving..." : "Save Config"}
          </button>
          <button onClick={handleAuthorize} disabled={authorizing || !config.client_id || !config.client_secret}>
            {authorizing ? "Authorizing..." : "Authorize"}
          </button>
        </div>

        {msg && <p className="platform-msg">{msg}</p>}
      </div>
    </div>
  );
}

// ── Activities Page ───────────────────────────────────────────

function ActivitiesPage() {
  const [activities, setActivities] = useState<Activity[]>([]);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [loading, setLoading] = useState(false);
  const [scanMsg, setScanMsg] = useState("");
  const [uploading, setUploading] = useState(false);
  const [fixing, setFixing] = useState(false);
  const [toast, setToast] = useState("");

  const [sourceFilter, setSourceFilter] = useState("");
  const [dateFrom, setDateFrom] = useState("");
  const [dateTo, setDateTo] = useState("");
  const [distMin, setDistMin] = useState("");
  const [distMax, setDistMax] = useState("");

  async function load() {
    setLoading(true);
    setScanMsg("");
    try {
      const scanResult = await invoke<string>("scan_gpx_dirs");
      if (!scanResult.includes("No new GPX")) {
        setScanMsg(scanResult);
      }
      const result = await invoke<Activity[]>("get_activities", {
        filter: {
          source: sourceFilter || null,
          dateFrom: dateFrom ? `${dateFrom}T00:00:00Z` : null,
          dateTo: dateTo ? `${dateTo}T23:59:59Z` : null,
          distanceMin: distMin ? parseFloat(distMin) * 1000 : null,
          distanceMax: distMax ? parseFloat(distMax) * 1000 : null,
        },
      });
      setActivities(result);
      setSelected(new Set());
    } catch (err) {
      console.error(err);
      setScanMsg(String(err));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load();
  }, []);

  async function handleDelete() {
    if (selected.size === 0) return;
    try {
      const result = await invoke<string>("delete_activities", { ids: Array.from(selected) });
      setToast(result);
      load();
    } catch (err) {
      setToast("Error: " + String(err));
    }
  }

  async function handleFixSources() {
    if (fixing) return;
    setFixing(true);
    try {
      const result = await invoke<string>("fix_local_sources");
      setToast(result);
      await load();
    } catch (err) {
      setToast("Error: " + String(err));
    } finally {
      setFixing(false);
    }
  }

  async function handleExport() {
    if (selected.size === 0) return;
    try {
      const folder = await open({ directory: true });
      if (!folder) return;
      const result = await invoke<string>("export_gpx", { ids: Array.from(selected), outputDir: folder });
      setToast(result);
    } catch (err) {
      setToast("Error: " + String(err));
    }
  }

  async function handleUpload() {
    if (selected.size === 0) return;
    if (selected.size > 200) {
      alert("Strava API limit: max 200 uploads per 15 minutes. Please select fewer activities.");
      return;
    }
    setUploading(true);
    try {
      const config = await invoke<StravaConfig>("get_strava_config");
      if (!config.authorized) {
        alert("Strava not authorized. Please go to Platforms page to authorize first.");
        setUploading(false);
        return;
      }
      const results = await invoke<string[]>("upload_to_strava", { ids: Array.from(selected) });
      alert(results.join("\n"));
    } catch (err) {
      alert(String(err));
    } finally {
      setUploading(false);
    }
  }

  function toggleOne(id: number) {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setSelected(next);
  }

  function toggleAll() {
    if (selected.size === activities.length && activities.length > 0) {
      setSelected(new Set());
    } else {
      setSelected(new Set(activities.map((a) => a.id)));
    }
  }

  return (
    <div className="activities-page">
      <div className="filter-bar">
        <select value={sourceFilter} onChange={(e) => setSourceFilter(e.target.value)}>
          <option value="">All Sources</option>
          <option value="codoon">咕咚</option>
          <option value="joyrun">悦跑圈</option>
          <option value="local">本地</option>
        </select>
        <input type="date" autoComplete="off" value={dateFrom} onChange={(e) => setDateFrom(e.target.value)} />
        <input type="date" autoComplete="off" value={dateTo} onChange={(e) => setDateTo(e.target.value)} />
        <input type="number" value={distMin} onChange={(e) => setDistMin(e.target.value)} placeholder="Min km" />
        <input type="number" value={distMax} onChange={(e) => setDistMax(e.target.value)} placeholder="Max km" />
        <button onClick={load}>Filter</button>
      </div>

      <div className="batch-bar">
        <label className="checkbox">
          <input type="checkbox" checked={activities.length > 0 && selected.size === activities.length} onChange={toggleAll} />
          Select All
        </label>
        <button onClick={handleDelete} disabled={selected.size === 0}>
          Delete ({selected.size})
        </button>
        <button onClick={handleFixSources} disabled={fixing}>
          {fixing ? "Fixing..." : "Fix Sources"}
        </button>
        <button onClick={handleExport} disabled={selected.size === 0}>
          Export GPX
        </button>
        <button onClick={handleUpload} disabled={selected.size === 0 || uploading}>
          {uploading ? "Uploading..." : "Upload"}
        </button>
      </div>

      <ImportLocalGpx onImport={load} />

      {toast && (
        <div className="toast" onClick={() => setToast("")}>
          {toast}
        </div>
      )}
      {scanMsg && <p className="scan-msg">{scanMsg}</p>}
      {loading && <p className="loading">Loading...</p>}

      <div className="activity-list">
        {activities.length === 0 && !loading && <p className="empty">No activities found.</p>}
        {activities.map((a) => (
          <div key={a.id} className="activity-row">
            <input type="checkbox" checked={selected.has(a.id)} onChange={() => toggleOne(a.id)} />
            <span className={`source-badge ${a.source}`}>{a.source}</span>
            <span className="sport">{a.sport_type || "Unknown"}</span>
            <span className="date">{fmtDate(a.start_time)}</span>
            <span className="distance">{fmtDist(a.distance_m)}</span>
            <span className="elevation">+{a.elevation_gain_m?.toFixed(0) ?? "-"}m</span>
            <span className="duration">{fmtDur(a.duration_sec)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

// ── Import Local GPX ──────────────────────────────────────────

function ImportLocalGpx({ onImport }: { onImport: () => void }) {
  const [paths, setPaths] = useState("");
  const [importing, setImporting] = useState(false);
  const [msg, setMsg] = useState("");

  async function doImport(filePaths: string[]) {
    if (filePaths.length === 0) return;
    setImporting(true);
    setMsg("Importing...");
    try {
      const result = await invoke<string>("import_local_gpx", { paths: filePaths });
      setMsg(result);
      setPaths("");
      onImport();
    } catch (err) {
      setMsg(String(err));
    } finally {
      setImporting(false);
    }
  }

  async function handleSelect() {
    try {
      const selected = await open({
        multiple: true,
        filters: [{ name: "GPX", extensions: ["gpx"] }],
      });
      if (selected && Array.isArray(selected)) {
        await doImport(selected);
      } else if (typeof selected === "string") {
        await doImport([selected]);
      }
    } catch (err) {
      setMsg(String(err));
    }
  }

  async function handleImport() {
    const list = paths.split("\n").map((s) => s.trim()).filter((s) => s.length > 0);
    await doImport(list);
  }

  return (
    <div className="import-box">
      <details>
        <summary>Import Local GPX</summary>
        <div className="import-actions">
          <button onClick={handleSelect} disabled={importing}>📁 Select GPX Files</button>
          <span className="import-or">or paste paths below</span>
        </div>
        <textarea rows={3} placeholder="Paste GPX file paths, one per line..." value={paths} onChange={(e) => setPaths(e.currentTarget.value)} />
        <button onClick={handleImport} disabled={importing || paths.trim().length === 0}>{importing ? "Importing..." : "Import"}</button>
        {msg && <span className="import-msg">{msg}</span>}
      </details>
    </div>
  );
}

// ── App ───────────────────────────────────────────────────────

function App() {
  const [view, setView] = useState<View>("activities");
  const [updateMsg, setUpdateMsg] = useState("");
  const [appVersion, setAppVersion] = useState("");

  useEffect(() => {
    async function init() {
      try {
        const ver = await getVersion();
        const suffix = import.meta.env.DEV ? "-dev" : "";
        setAppVersion(ver + suffix);
      } catch {
        // ignore in dev mode
      }
      try {
        const update = await check();
        if (update) {
          setUpdateMsg(`Update v${update.version} available. Click to install.`);
        }
      } catch {
        // updater not available in dev mode, ignore
      }
    }
    init();
  }, []);

  async function installUpdate() {
    try {
      const update = await check();
      if (update) {
        setUpdateMsg("Downloading update...");
        await update.downloadAndInstall();
      }
    } catch (err) {
      setUpdateMsg("Update failed: " + String(err));
    }
  }

  return (
    <main className="container">
      <h1>RunBridge v{appVersion || "dev"}</h1>
      {updateMsg && (
        <div className="update-banner" onClick={installUpdate}>
          {updateMsg}
        </div>
      )}
      <nav className="nav-tabs">
        <button className={view === "sync" ? "active" : ""} onClick={() => setView("sync")}>Sync</button>
        <button className={view === "activities" ? "active" : ""} onClick={() => setView("activities")}>Activities</button>
        <button className={view === "platforms" ? "active" : ""} onClick={() => setView("platforms")}>Platforms</button>
      </nav>
      {view === "sync" ? <SyncPage /> : view === "activities" ? <ActivitiesPage /> : <PlatformsPage />}
    </main>
  );
}

export default App;
