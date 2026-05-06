import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
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

type View = "sync" | "activities";

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
  return d.toLocaleString("zh-CN", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

// ── Sync Page ─────────────────────────────────────────────────

function SyncPage() {
  const [platform, setPlatform] = useState<"codoon" | "joyrun">("codoon");
  const [log, setLog] = useState("");
  const [loading, setLoading] = useState(false);

  // codoon
  const [cdMobile, setCdMobile] = useState("");
  const [cdPassword, setCdPassword] = useState("");
  const [cdUseToken, setCdUseToken] = useState(false);

  // joyrun
  const [jrPhone, setJrPhone] = useState("");
  const [jrCode, setJrCode] = useState("");
  const [jrUseSid, setJrUseSid] = useState(false);

  async function handleSync() {
    setLoading(true);
    setLog("Starting sync...\n");
    try {
      const result =
        platform === "codoon"
          ? await invoke("sync_codoon", {
              mobile: cdMobile,
              password: cdPassword,
              use_token: cdUseToken,
            })
          : await invoke("sync_joyrun", {
              phone: jrPhone,
              code: jrCode,
              use_sid: jrUseSid,
            });
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
        <button
          className={platform === "codoon" ? "active" : ""}
          onClick={() => setPlatform("codoon")}
        >
          咕咚 (Codoon)
        </button>
        <button
          className={platform === "joyrun" ? "active" : ""}
          onClick={() => setPlatform("joyrun")}
        >
          悦跑圈 (Joyrun)
        </button>
      </div>

      {platform === "codoon" && (
        <>
          <div className="form-group">
            <label>手机号 / Refresh Token</label>
            <input
              type="text"
              value={cdMobile}
              onChange={(e) => setCdMobile(e.currentTarget.value)}
              placeholder={cdUseToken ? "refresh_token" : "mobile number"}
            />
          </div>
          <div className="form-group">
            <label>密码 / User ID</label>
            <input
              type="password"
              value={cdPassword}
              onChange={(e) => setCdPassword(e.currentTarget.value)}
              placeholder={cdUseToken ? "user_id" : "password"}
            />
          </div>
          <div className="form-group checkbox">
            <label>
              <input
                type="checkbox"
                checked={cdUseToken}
                onChange={(e) => setCdUseToken(e.currentTarget.checked)}
              />
              使用 Refresh Token 登录
            </label>
          </div>
        </>
      )}

      {platform === "joyrun" && (
        <>
          <div className="form-group">
            <label>手机号 / UID</label>
            <input
              type="text"
              value={jrPhone}
              onChange={(e) => setJrPhone(e.currentTarget.value)}
              placeholder={jrUseSid ? "uid" : "phone number"}
            />
          </div>
          <div className="form-group">
            <label>验证码 / SID</label>
            <input
              type="text"
              value={jrCode}
              onChange={(e) => setJrCode(e.currentTarget.value)}
              placeholder={jrUseSid ? "sid" : "SMS code"}
            />
          </div>
          <div className="form-group checkbox">
            <label>
              <input
                type="checkbox"
                checked={jrUseSid}
                onChange={(e) => setJrUseSid(e.currentTarget.checked)}
              />
              使用 UID + SID 登录
            </label>
          </div>
        </>
      )}

      <button onClick={handleSync} disabled={loading} className="sync-btn">
        {loading ? "Syncing..." : "Start Sync"}
      </button>

      {log && (
        <div className="log-box">
          <pre>{log}</pre>
        </div>
      )}
    </div>
  );
}

// ── Activities Page ───────────────────────────────────────────

function ActivitiesPage() {
  const [activities, setActivities] = useState<Activity[]>([]);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [loading, setLoading] = useState(false);

  // filters
  const [sourceFilter, setSourceFilter] = useState("");
  const [dateFrom, setDateFrom] = useState("");
  const [dateTo, setDateTo] = useState("");
  const [distMin, setDistMin] = useState("");
  const [distMax, setDistMax] = useState("");

  async function load() {
    setLoading(true);
    try {
      const result = await invoke<Activity[]>("get_activities", {
        filter: {
          source: sourceFilter || null,
          date_from: dateFrom ? `${dateFrom}T00:00:00Z` : null,
          date_to: dateTo ? `${dateTo}T23:59:59Z` : null,
          distance_min: distMin ? parseFloat(distMin) * 1000 : null,
          distance_max: distMax ? parseFloat(distMax) * 1000 : null,
        },
      });
      setActivities(result);
      setSelected(new Set());
    } catch (err) {
      console.error(err);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load();
  }, []);

  async function handleDelete() {
    if (selected.size === 0) return;
    if (!confirm(`Delete ${selected.size} activities?`)) return;
    try {
      await invoke("delete_activities", { ids: Array.from(selected) });
      load();
    } catch (err) {
      alert(String(err));
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
        <select
          value={sourceFilter}
          onChange={(e) => setSourceFilter(e.target.value)}
        >
          <option value="">All Sources</option>
          <option value="codoon">咕咚</option>
          <option value="joyrun">悦跑圈</option>
        </select>
        <input
          type="date"
          value={dateFrom}
          onChange={(e) => setDateFrom(e.target.value)}
        />
        <input
          type="date"
          value={dateTo}
          onChange={(e) => setDateTo(e.target.value)}
        />
        <input
          type="number"
          value={distMin}
          onChange={(e) => setDistMin(e.target.value)}
          placeholder="Min km"
        />
        <input
          type="number"
          value={distMax}
          onChange={(e) => setDistMax(e.target.value)}
          placeholder="Max km"
        />
        <button onClick={load}>Filter</button>
      </div>

      <div className="batch-bar">
        <label className="checkbox">
          <input
            type="checkbox"
            checked={
              activities.length > 0 && selected.size === activities.length
            }
            onChange={toggleAll}
          />
          Select All
        </label>
        <button onClick={handleDelete} disabled={selected.size === 0}>
          Delete ({selected.size})
        </button>
        <button disabled title="Coming soon">Upload</button>
      </div>

      {loading && <p className="loading">Loading...</p>}

      <div className="activity-list">
        {activities.length === 0 && !loading && (
          <p className="empty">No activities found.</p>
        )}
        {activities.map((a) => (
          <div key={a.id} className="activity-row">
            <input
              type="checkbox"
              checked={selected.has(a.id)}
              onChange={() => toggleOne(a.id)}
            />
            <span className={`source-badge ${a.source}`}>{a.source}</span>
            <span className="sport">{a.sport_type || "Unknown"}</span>
            <span className="date">{fmtDate(a.start_time)}</span>
            <span className="distance">{fmtDist(a.distance_m)}</span>
            <span className="elevation">
              +{a.elevation_gain_m?.toFixed(0) ?? "-"}m
            </span>
            <span className="duration">{fmtDur(a.duration_sec)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

// ── App ───────────────────────────────────────────────────────

function App() {
  const [view, setView] = useState<View>("activities");

  return (
    <main className="container">
      <h1>RunBridge</h1>
      <nav className="nav-tabs">
        <button
          className={view === "sync" ? "active" : ""}
          onClick={() => setView("sync")}
        >
          Sync
        </button>
        <button
          className={view === "activities" ? "active" : ""}
          onClick={() => setView("activities")}
        >
          Activities
        </button>
      </nav>
      {view === "sync" ? <SyncPage /> : <ActivitiesPage />}
    </main>
  );
}

export default App;
