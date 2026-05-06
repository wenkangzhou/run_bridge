# RunBridge

RunBridge 是一个桌面应用，用于从运动平台（咕咚、悦跑圈）拉取 GPX 轨迹到本地，统一管理、筛选和批量操作。

> **当前状态**：MVP，支持咕咚/悦跑圈数据拉取、GPX 解析、活动列表展示与筛选、批量删除。上传功能待开发。

---

## 功能

- **Sync 页面**：从咕咚或悦跑圈拉取运动数据，自动下载 GPX 文件
- **Activities 页面**：
  - 列表展示所有已导入活动（运动类型、日期、距离、海拔、时长）
  - 按来源/时间范围/距离范围筛选
  - 批量选择 + 删除
  - 批量上传（占位，即将支持）

---

## 项目结构

```
RunBridge/
├── src/                     # React + Vite 前端
│   ├── App.tsx
│   └── App.css
├── src-tauri/
│   ├── src/lib.rs           # Rust 后端（GPX 解析、SQLite、调用 Python 脚本）
│   └── tauri.conf.json
├── running_page/            # 内置 running_page Python 脚本（仅用于拉取）
│   ├── run_page/
│   │   ├── codoon_sync.py
│   │   ├── joyrun_sync.py
│   │   └── ...
│   └── GPX_OUT/             # 脚本临时输出目录
├── requirements.txt         # Python 依赖
└── package.json             # npm 依赖（含 @tauri-apps/cli）
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

## 使用

1. 打开 **Sync** 页面
2. 选择平台（咕咚 / 悦跑圈）
3. 输入登录凭证：
   - **咕咚**：手机号 + 密码（或 refresh_token + user_id）
   - **悦跑圈**：手机号 + 短信验证码（或 UID + SID）
4. 点击 **Start Sync**，等待拉取完成
5. 切换到 **Activities** 页面查看列表
6. 用筛选栏过滤，勾选后批量删除

---

## 构建

```bash
npx tauri build
```

---

## 常见问题

### `error: no such command: tauri`

请使用 `npx tauri dev`，不要运行 `cargo tauri dev`。

### 编译报错 `failed to read plugin permissions`

清理缓存后重试：

```bash
rm -rf src-tauri/target
npx tauri dev
```

---

## 已知限制 & 下一步

- [ ] **上传功能**：批量上传到 Strava / 其他平台（即将支持）
- [ ] **平台授权管理**：独立的授权页面，持久化 token
- [ ] **实时日志**：同步过程中流式输出进度
- [ ] **凭证安全存储**：接入系统 keyring
