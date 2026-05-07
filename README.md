# RunBridge

RunBridge 是一个桌面应用，用于从运动平台（咕咚、悦跑圈）拉取 GPX 轨迹到本地，统一管理、筛选，并批量上传/导出到 Strava。

---

## 功能

### Sync 页面
- 从**咕咚**或**悦跑圈**拉取运动数据，自动下载 GPX 文件
- 支持多种登录方式：手机号/密码、Token、UID+SID
- **账号记忆**：自动保存/填充登录凭证
- **可取消**：同步过程中可点击 Cancel 中断
- **增量同步**：Python 脚本全量抓取，Rust 端按 `external_id` 去重，已存在的记录不会重复导入

### Activities 页面
- 列表展示所有已导入活动（来源、运动类型、日期、距离、海拔、时长）
- **自动扫描**：打开页面时自动扫描 `GPX_OUT/` 和 `data/gpx/`
- **筛选**：按来源（咕咚/悦跑圈/本地）、时间范围、距离范围过滤
- **批量操作**：
  - 全选 / 批量删除（软删除）
  - **批量导出 GPX**（弹出目录选择器）
  - **批量上传到 Strava**
  - **Fix Sources**：自动修复被错误标记为 `local` 的活动来源
- **脏数据过滤**：导入时自动跳过 distance = 0 且 elevation = 0 的空记录

### Platforms 页面
- **Strava OAuth 授权**：内置本地 HTTP server 捕获 callback，自动换取并持久化 refresh_token
- Client ID / Client Secret 配置保存到 SQLite
- 授权状态实时显示

---

## 项目结构

```
RunBridge/
├── src/                     # React + Vite + TypeScript 前端
│   ├── App.tsx
│   └── App.css
├── src-tauri/
│   ├── src/lib.rs           # Rust 后端（GPX 解析、SQLite、Strava API、Python 调用）
│   └── tauri.conf.json
├── running_page/            # 内置 running_page Python 脚本（打包时嵌入）
│   ├── run_page/
│   │   ├── codoon_sync.py
│   │   ├── joyrun_sync.py
│   │   └── gpx_to_strava_sync.py
│   └── GPX_OUT/             # 脚本 GPX 输出目录（已加入 .gitignore）
├── data/                    # SQLite 数据库 + 本地 GPX 存储（已加入 .gitignore）
├── requirements.txt         # Python 依赖
└── package.json             # npm 依赖
```

---

## 前置条件

| 组件 | 要求 |
|------|------|
| Node.js | v18+ |
| Rust | 最新 stable（rustup 安装）|
| Python | 3.10+ |

---

## 安装

```bash
# 1. 安装前端依赖
npm install

# 2. 安装 Python 依赖
pip install -r requirements.txt
```

> **不需要** `cargo install tauri-cli`，Tauri CLI 已通过 npm 安装。

---

## 开发运行

```bash
npx tauri dev
```

首次编译 Rust 后端约需 1~2 分钟。

---

## 本地打包

```bash
npx tauri build --ignore-version-mismatches
```

产物路径：
- **macOS**: `src-tauri/target/release/bundle/dmg/RunBridge_0.1.0_aarch64.dmg`
- **Windows**: `src-tauri/target/release/bundle/msi/` + `src-tauri/target/release/bundle/nsis/`

> 首次打开未签名应用时，macOS 会提示"无法验证开发者"，需在**系统设置 > 隐私与安全性**中允许。

---

## 使用流程

### 1. 配置 Strava（可选，如需上传）
1. 打开 **Platforms** 页面
2. 输入 Strava App 的 Client ID 和 Client Secret
3. 点击 **Authorize**，在浏览器中完成 OAuth 授权

### 2. 同步运动数据
1. 打开 **Sync** 页面
2. 选择平台（咕咚 / 悦跑圈）
3. 输入登录凭证：
   - **咕咚**：手机号 + 密码（或 refresh_token + user_id）
   - **悦跑圈**：手机号 + 短信验证码（或 UID + SID）
4. 点击 **Start Sync**，等待拉取完成
5. 账号会自动保存，下次打开自动填充

### 3. 管理活动
1. 切换到 **Activities** 页面，自动扫描并展示列表
2. 用筛选栏按来源/时间/距离过滤
3. 勾选活动后进行批量操作：
   - **Delete**：软删除选中记录
   - **Fix Sources**：自动识别 GPX 内容并修正来源（codoon/joyrun）
   - **Export GPX**：选择本地目录导出 GPX 文件
   - **Upload**：批量上传到 Strava（15 分钟限制 200 条）

---

## 自动更新

应用启动时会自动检查 GitHub Release 是否有新版本，并在顶部显示更新横幅。点击即可下载安装。

---

## 技术栈

- **前端**：React 19 + Vite + TypeScript
- **桌面框架**：Tauri v2
- **后端**：Rust（`gpx`、`rusqlite`、`reqwest`、`tokio`）
- **数据同步**：Python 3（`codoon_sync.py`、`joyrun_sync.py`）
- **数据库**：SQLite（`data/runbridge.db`）
