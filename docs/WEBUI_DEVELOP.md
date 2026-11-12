# RKP Mini WebUI 开发方案

> 状态：**已实现**（2026-11 三命令拆分后）。本文保留为设计记录；实现现状以下面这段为准。
> 读者：接手维护的新 Agent 会话。
> 先读：仓库根 `AGENTS.md`（架构、约束与验证命令），再读本文。
>
> **实现现状（与下文设计稿的差异）**
>
> - 两个独立页面：`webui/src/apps/issuer/`（签发端 + 发送端同步）与 `webui/src/apps/deployer/`
>   （部署端），共享代码在 `webui/src/shared/`；`entry.ts` 只接受显式 `--mode issuer|deployer`。
> - 三个二进制：`rkp-mini-issuer`、`rkp-mini-sync`（两个模块共用同一构建产物）、
>   `rkp-mini-deployer`（仅本地部署）。模块 id 为 `rkp_mini_issuer` / `rkp_mini_deployer`，
>   数据根仍是 `/data/misc/keystore/rkp-mini` 与 `/data/adb/rkp_sync`。
> - 页面调用方式：`bin/rkp-mini-sync --role <sender|receiver> --config <path> --json <cmd>` 与
>   `bin/rkp-mini-deployer --role receiver --config <path> --json <cmd>`（`deploy` 子命令已取消）。
> - 组合任务（fetch + apply）由模块脚本 `modules/deployer/run.sh` 编排，结果以
>   `tmp/sync-task.last.steps` 八段快照返回；页面不再解析 Rust 侧 `run` 信封。
> - 后台脚本已移入模块源码树（`modules/*/webroot/runner/`），Vite 只构建页面（`publicDir: false`）。
> - 构建产物为 `webui/dist/issuer/` 与 `webui/dist/deployer/`；ZIP 为
>   `dist/rkp_mini_issuer-v*.zip` 与 `dist/rkp_mini_deployer-v*.zip`。
> - 签发端页面分为“状态 / 签发 / 同步 / 更多”四个 Tab，用 `MiuixNavigationBar` 固定在底部；当前 Tab 只是组件状态，
>   不写 URL/历史，返回键仍直接关闭 WebUI。原设置弹层成为“更多”页（`MoreTab.vue`）。状态卡按 KernelSU Miuix 首页
>   `StatusCard`（`HomeMiuix.kt`）着色：按状态染色的卡片底 + 右下角裁切的 110px Material Rounded 图标。
>   部署端仍是单页，后续按同一方式改造。
> - 下文的 `rkp-mini` / `rkp-sync` 命令名、`dist/`、`dist-receiver/` 与 `rkp_mini`/`rkp_sync`
>   模块 id 均为设计阶段的历史写法，按上面这段替换。

---

## 0. 一句话

在 `webui/` 下用 **Vue 3 + TypeScript + Vite + Bun + `miuix-vue`** 实现 KernelSU WebUI，构建产物进入模块 `webroot/`；页面通过 `ksu.exec` 调用 `rkp-mini-issuer` / `rkp-mini-sync` / `rkp-mini-deployer` 读取状态、触发签发、同步与部署，签发与组合任务都在**脱离页面的后台进程**中执行，结果写文件、页面轮询。

---

## 1. 已定决策（不要再重新论证）

| 决策 | 结论 | 理由 |
|---|---|---|
| 框架 | Vue 3.5 + TS | `miuix-vue` 是现成的 Miuix 移植，零移植成本 |
| 组件库 | `miuix-vue@0.1.1`（精确锁定） | 令牌与样式 1:1 取自 Miuix `Colors.kt` / `TextStyles.kt` |
| 构建 | Vite 8 + `@vitejs/plugin-vue`，Bun 负责装包/脚本 | 实测可用，产物小 |
| 样式 | 直接用库的 `miuix-vue/style.css` + 少量应用层 CSS | 不引 Tailwind、不引其它 UI 库 |
| 类型 | tsconfig `paths` 兜底（见附录 B） | 上游 0.1.1 的类型入口路径是坏的 |
| 路由 | 无路由。单页（签发端带底部 Tab，仅切换组件状态）+ `MiuixBottomSheet` / `MiuixDialog` | KernelSU 的返回键没有 SPA 历史，扁平结构最稳 |
| 签发执行 | 后台脚本 + 状态文件 + 轮询 | `ksu.exec` 无超时，页面销毁会丢回调 |
| 配置 | v1 只读（展示 TOML 原文），不提供编辑 | 避免写坏 `config.toml` |
| Action | 保留 `action.sh` | KernelSU 的 Action 与 Open 按钮共存，见 `ModuleMiuix.kt` |
| 证书详情 | 只展示 `status.json` 已有字段 | 展开完整链需要新增二进制命令，v1 不做 |

---

## 2. 已验证事实（不要再重复调研）

以下均在 2026-11-09 的会话中实测或读源码确认。

### 2.1 前端构建（实测）

测试页包含 12 个 `miuix-vue` 组件 + 2 个图标 + 主题，`bun install && bunx vite build`（Vue 3.5 / Vite 8.3.7 / Bun 1.3.14）：

| 产物 | 原始 | gzip |
|---|---|---|
| JS | 240.39 kB | **83.89 kB** |
| CSS（整库样式表） | 35.78 kB | **5.72 kB** |
| index.html | 0.41 kB | 0.28 kB |

- 图标入口 `miuix-vue/icons` 源文件 1.3 MB，但**tree-shaking 正常**（只引 2 个图标，bundle 未膨胀）。
- `miuix-vue.css` 中**没有** `url()`、`@import`、`http(s)://` → 完全离线，无远程字体/图片。
- 生产产物中**没有** `eval(` / `new Function` → CSP 可以只写 `script-src 'self'`。
- 硬性依赖：`vue ^3.4`、`motion-v ^2`（peer）。`motion-v` 会带 `framer-motion` 与 `@vueuse/core`，这就是 84 kB gzip 的主要来源，属于 Miuix 弹簧手感的成本。

### 2.2 上游缺陷（0.1.1）

`package.json` 的 `types` 与 `exports["."].types` 指向 `./dist/index.d.ts`，但发布包里不存在；真实位置是 `dist/src/index.d.ts`（`icons` 同理为 `dist/src/icons/extended/index.d.ts`）。直接 `tsc` 会得到：

```
error TS7016: Could not find a declaration file for module 'miuix-vue'
```

修法见附录 B，**已实测 `tsc` exit 0**。

### 2.3 KernelSU WebUI 事实（读 manager 源码确认）

| 事实 | 影响 |
|---|---|
| 页面 URL 是 `https://mui.kernelsu.org/index.html`，`webroot/` 由 `WebViewAssetLoader` + root 读取映射 | 同源 HTTPS，ES module / fetch / Cache API 可用；相对路径资源正常 |
| `webView.setBackgroundColor(Color.TRANSPARENT)` | **页面必须自己画背景**，否则透出 manager 的底色 |
| `allowFileAccess = false`、`domStorageEnabled = true` | `localStorage` 可用；不能读本地文件（用 `exec`/`io`） |
| `textZoom` 跟随系统字体缩放 | 用 px/rem 写字号都会缩放，无障碍天然好于 canvas |
| 页面没有历史记录时，返回键直接关闭 WebUI | 不要设计需要“返回上一层”的导航；弹层自行提供关闭按钮 |
| `ksu.exec(cmd, optsJson, cbName)` 回调式，**无超时** | 长任务不能靠它等待；短命令需自加超时 |
| `dispose()` 只关 manager 自己持有的 shell | 后台进程要 `setsid` 脱离，否则可能随 manager 被杀 |
| 模块存在 `webroot/index.html` 才出现 Open 按钮；Action 按钮与它共存 | 两个入口都保留 |
| WebView 背景透明、由 `WebUIScreen` 加 inset padding | 不要调用 `enableEdgeToEdge(true)`，按默认 padding 布局 |

### 2.4 二进制现有契约（读源码确认）

- 模块 id 固定 `rkp_mini`，二进制 `/data/adb/modules/rkp_mini/bin/rkp-mini`。
- 数据根 `DATA_DIR = /data/misc/keystore/rkp-mini`；配置 `config.toml`；默认输出 `output/keybox.xml`；日志 `logs/issue-<unix>.log`；临时目录 `tmp/`。
- `status` 只做本地检查，会写 `status.json`（0600）与 `status.txt`；`--update-description` 额外改 `module.prop`。
- **`issue` 成功不会刷新 `status.json`，也不改描述**；失败时会把描述改成错误摘要。
- `issue` 每次生成新的 P-256 密钥；失败保留旧 keybox；不是多文件事务。
- `issue` 未带 `--acknowledge` 时退出码 2。

---

## 3. 目标与非目标

### 目标（v1）

1. 在 KernelSU 模块卡片里打开 WebUI，看到当前 keybox 状态与设备前置条件。
2. 勾选确认后触发一次真实签发，展示阶段、耗时、日志与结果。
3. 签发成功后刷新状态与模块描述；失败时解释阶段/错误码/`ta_status`。
4. 展示证书详情（链长度、叶子主题、有效期、密钥匹配、指纹/路径），提供复制。
5. 只读展示 `config.toml`、最近日志、版本信息。
6. 打包进 `dist/rkp_mini-vX.Y.Z.zip`，ZIP 根目录含 `module.prop`、脚本、`bin/`、`webroot/`。

### 非目标（v1 明确不做）

- 编辑 `config.toml`、修改 server/output/fingerprint。
- 展示 `keybox.xml`、私钥、完整证书链内容、导出凭证。
- 定时/自动签发（`auto_issue_days` 仍是保留字段）。
- Play Integrity 结果承诺。
- V1/AIDL 协议回退、daemon、远程下发私钥。
- WebUI 内切换模块版本、卸载、重启。

---

## 4. 运行时契约

### 4.1 路径常量（前端集中在一处）

> 现在的实现：路径来自各应用的 `config.ts`（`appConfig`），见 `webui/src/apps/*/config.ts`。

```ts
// 签发端（webui/src/apps/issuer/config.ts）
const MODULE_ID = 'rkp_mini_issuer'
const MODULE_DIR = '/data/adb/modules/rkp_mini_issuer'  // 优先用 moduleInfo().moduleDir
const BIN = `${MODULE_DIR}/bin/rkp-mini-issuer`
const SYNC_BIN = `${MODULE_DIR}/bin/rkp-mini-sync`
const DATA = '/data/misc/keystore/rkp-mini'

// 部署端（webui/src/apps/deployer/config.ts）
//   MODULE_ID = 'rkp_mini_deployer', MODULE_DIR = '/data/adb/modules/rkp_mini_deployer'
//   DEPLOY_BIN = `${MODULE_DIR}/bin/rkp-mini-deployer`, DATA = '/data/adb/rkp_sync'

const TMP = `${DATA}/tmp`
const CONFIG = `${DATA}/config.toml`
const STATUS_JSON = `${DATA}/status.json`
const LOGS = `${DATA}/logs`
```

`output_dir` 可能被用户改过，**不要硬编码输出路径**；一律显示 `status.keybox.path`。

### 4.2 命令与输出

| 命令 | 用途 | stdout |
|---|---|---|
| `${BIN} status --json` | 读状态 | 单行 `{"ok":true,"data":{...}}` |
| `${BIN} status --json --update-description` | 刷新状态并同步模块描述 | 同上 |
| `${BIN} issue --acknowledge --json` | 签发（仅由 runner 调用） | 成功 `{"ok":true,"data":{...}}`；失败 `{"ok":false,"error":{...}}` |
| `${BIN} --version` | 版本 | `rkp-mini-issuer 0.2.0` |
| `cat ${CONFIG}` | 只读配置 | TOML 文本 |
| `ls -1t ${LOGS}` | 日志列表 | 文件名，最新在前 |
| `tail -n 200 ${LOGS}/<name>` | 日志内容 | 文本 |

解析规则：**只解析 stdout**，取最后一行非空内容 `JSON.parse`；stderr 只用于诊断展示。`ctx` 前缀（`cd`/`export`）由 bridge 拼装，调用方不要再包 `cd`。

`status` 的 `data`：

```json
{
  "generated_at": 1762700000,
  "device": {
    "android_arm64": true, "root": true, "qualcomm": true,
    "library_present": true, "device_present": true, "fingerprint": "..."
  },
  "blockers": [],
  "keybox": {
    "path": "/data/misc/keystore/rkp-mini/output/keybox.xml",
    "exists": true, "valid": true, "certificates": 3,
    "leaf_subject": "CN=...", "not_after": 1791000000,
    "not_after_utc": "2027-02-04 00:00:00 UTC",
    "days_left": 87, "key_match": true, "error": null
  },
  "description": "✅ RKP 2027-02-04（87天）"
}
```

`issue` 成功 `data`：`keybox`、`chain_certificates`、`leaf_subject`、`leaf_not_after`、`leaf_not_after_utc`、`device_id`、`csr_len`、`frame_layout`、`ta_status`。

`issue` 失败 `error`：`stage`、`code`、`message`，可选 `ta_status`、`head_hex`。

blocker 取值（原样字符串，UI 映射中文）：

`android_arm64_required`、`root_required`、`qualcomm_required`、`soc_unknown`、`soc_conflicting`、`keymint_library_missing`、`smcinvoke_missing`、`fingerprint_missing`。

### 4.3 退出码

| code | 阶段 | UI 文案 |
|---|---|---|
| 0 | 成功 | 已签发 |
| 1 | generic | 未知错误 |
| 2 | config | 配置/用法错误 |
| 3 | prereq | 前置条件不满足 |
| 4 | challenge / server | 网络或服务端失败（HTTP） |
| 5 | ta / csr | 设备 TA 签名或 CSR 校验失败 |
| 6 | chain | 证书链校验失败 |
| 7 | write | 写入文件失败 |

`stage` 中文映射：`config` 配置、`prereq` 前置条件、`challenge` 获取挑战、`server` 证书服务、`ta` TA 签名、`csr` CSR 校验、`chain` 证书链、`write` 写入、`generic` 其他。

### 4.4 签发执行（核心设计）

**不要**用 `exec` 等待 `issue`。用静态脚本 + 后台运行：

`webui/public/runner/issue.sh`（构建后位于 `webroot/runner/issue.sh`，0644，用 `sh` 读执行）：

```sh
#!/system/bin/sh
# rkp-mini WebUI issue runner. 由 WebUI 以 root 启动，脱离页面生命周期。
set -u
DATA=/data/misc/keystore/rkp-mini
TMP=$DATA/tmp
BIN=/data/adb/modules/rkp_mini/bin/rkp-mini

mkdir -p "$TMP" || exit 9
mkdir "$TMP/issue.lock" 2>/dev/null || exit 9   # 已有任务在跑
trap 'rmdir "$TMP/issue.lock" 2>/dev/null' EXIT

rm -f "$TMP/issue.last.json" "$TMP/issue.last.err" "$TMP/issue.last.code"
"$BIN" issue --acknowledge --json >"$TMP/issue.last.json" 2>"$TMP/issue.last.err"
code=$?
printf '%s\n' "$code" >"$TMP/issue.last.code"
```

启动（一次 `exec`，立即返回）：

```ts
const cmd =
  `setsid sh '${MODULE_DIR}/webroot/runner/issue.sh' </dev/null >/dev/null 2>&1 &`
await exec(cmd)
```

若设备无 `setsid`（`command -v setsid` 为空），回退 `nohup sh '…/issue.sh' </dev/null >/dev/null 2>&1 &`，并在调试日志里记录命中的分支。

状态机（页面每次挂载/轮询都走同一套判断）：

| 观察 | 结论 |
|---|---|
| `tmp/issue.lock` 存在、无 `issue.last.code` | 任务进行中 → 继续轮询 |
| `tmp/issue.last.code` 存在 | 任务结束 → 读 json/err，展示结果 |
| 两者都不存在 | 空闲 |
| lock 存在但 mtime > 10 min 且无 code | 残留锁 → 提供“清除锁”（`rmdir`） |

轮询：600 ms 一次，10 s 后降到 1 s；上限 120 s 后提示“结果未知，请查看日志”。只读文件，不杀进程（没有取消能力，也不要假装有）。残留锁判定用 `stat -c %Y "$TMP/issue.lock"` 与当前时间比较。

页面重进时的恢复：结果文件仍在 → 展示“上次签发结果”卡片，直到用户关闭。

### 4.5 刷新顺序

任务结束后**无论成败**都执行一次：

```
${BIN} status --json --update-description
```

原因：`issue` 只写 keybox 与描述，不写 `status.json`；成功时描述也不会自动变。刷新后重新渲染状态卡。签发运行期间不要并发跑 `status`。

---

## 5. 仓库改动清单

### 新增

```
webui/
  package.json
  bun.lock                  # 提交
  vite.config.ts
  tsconfig.json
  index.html
  public/runner/issue.sh
  src/main.ts
  src/App.vue
  src/env.d.ts
  src/styles/app.css
  src/lib/ksu.ts            # bridge 封装（超时、mock）
  src/lib/rkp.ts            # 命令 + JSON 类型 + 解析
  src/lib/runner.ts         # 启动/轮询/恢复
  src/lib/format.ts         # 日期、天数、指纹截断
  src/components/StatusCard.vue
  src/components/CheckCard.vue
  src/components/IssueSheet.vue
  src/components/ResultSheet.vue
  src/components/DetailSheet.vue
  src/components/SettingsSheet.vue
  src/dev/mockKsu.ts        # 仅 dev：fixture 驱动
  src/dev/fixtures/*.json
docs/WEBUI_DEVELOP.md       # 本文
```

### 修改

| 文件 | 改动 |
|---|---|
| `build.sh` | 构建 webui，拷贝 `webui/dist/.` 到 `dist/module/webroot/`，缺失即失败 |
| `.gitignore` | 增加 `/webui/node_modules`、`/webui/dist` |
| `AGENTS.md` | 把 “No ... WebUI” 改为已包含 WebUI，并记录构建前置（Bun/Vite） |
| `README.md` | 安装说明补一句：模块卡片里的“打开”进入 WebUI |
| `module/customize.sh` | **不改权限逻辑**；KernelSU 自行处理 `webroot` 的权限与 SELinux 上下文 |

`module/` 保持纯源码，不写构建产物；`dist/module/` 是暂存目录。

---

## 6. 构建与打包

### 6.1 `webui/package.json`

```json
{
  "name": "rkp-mini-webui",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vue-tsc --noEmit && vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "kernelsu": "3.3.0",
    "miuix-vue": "0.1.1",
    "motion-v": "^2.11.1",
    "vue": "^3.5.35"
  },
  "devDependencies": {
    "@vitejs/plugin-vue": "^6.0.7",
    "typescript": "~6.0.3",
    "vite": "^8.0.16",
    "vue-tsc": "^3.3.3"
  }
}
```

### 6.2 `webui/vite.config.ts`

```ts
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  base: './',                        // KernelSU 用相对路径加载
  build: {
    outDir: 'dist',
    target: 'es2020',
    cssCodeSplit: false,             // 单 CSS，便于确认 MIME
  },
})
```

### 6.3 `build.sh`

在 `--- Package ---` 之前插入：

```sh
# --- WebUI --------------------------------------------------------------------
command -v bun >/dev/null 2>&1 || { echo "! bun is required to build the WebUI" >&2; exit 1; }
echo "- Building WebUI"
( cd webui && bun install --frozen-lockfile && bun run build )
[ -f webui/dist/index.html ] || { echo "! webui build produced no index.html" >&2; exit 1; }
```

打包段增加：

```sh
mkdir -p dist/module/webroot
cp -R webui/dist/. dist/module/webroot/
```

ZIP 仍用 `( cd dist/module && zip -rq ... . )`，自动包含 `webroot/`。验证段建议加一条：

```sh
unzip -l "$ZIP" | grep -q 'webroot/index.html' || { echo "! webroot missing" >&2; exit 1; }
```

（`grep` 仅在脚本内使用，不受仓库工具策略限制。）

### 6.4 页面骨架约束

- `index.html` 加 CSP：
  `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'none'`
  （样式必须留 `'unsafe-inline'`：Vue/motion 会写内联 style。）
- `html, body, #app { height: 100%; background: var(--m-color-background); }`——WebView 背景透明。
- `<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">`。
- 不要引入任何远程字体/图片/CDN。

---

## 7. UI 规格

### 7.1 结构

签发端：顶栏 + 四个 Tab（每个 Tab 一个 `MiuixScrollArea`，`v-show` 切换以保留各自滚动位置）+ 底部导航，无路由：

```
MiuixTopAppBar(title="RKP-Mini · 签发端")
状态  StatusCard（按状态着色 + 角标图标；保留大号剩余天数；右上角状态即刷新入口，签发中显示“签发中 + 用时”；读取/刷新失败进警告条）、
      AutoStatusRow（点击跳到签发 / 同步 Tab；自动签发：issuer status 的 auto_issue.days > 0 且 auto-loop.sh 在运行；
      自动推送：sync status 的 enabled + risk_ack_current + auto_push + loop_running）、CheckCard
签发  IssueStatusCard（同 SyncStatusCard 样式：有效期至 / 剩余天数 / 自动签发；即将过期/过期/无效时警告条；
      签发任务与上次结果在卡内；右上角状态即刷新入口，签发中显示“签发中 + 用时”；点击 → DetailSheet）、
      不可签发原因、“立即签发”、“签发设置”、免责脚注
同步  SyncStatusCard（存储方式 / 同步时间 / 自动推送；异常时警告条；点击 → SyncDetailSheet；右上角状态即刷新入口，
      点击经 runner `check` 运行 `check-remote`，任务进行中在同一位置显示“检查中/推送中 + 用时”）、“立即推送”或“启用”、“分享”、“同步设置”
更多  MoreTab：外观、日志入口、只读配置、关于（每次切入重新读取）
MiuixNavigationBar  状态 / 签发 / 同步 / 更多
MiuixSnackbarHost   （挂载一次；上移到导航栏之上）
IssueSheet / ResultSheet / DetailSheet / IssueSettingsSheet / LogsSheet / 同步相关弹层（全局，覆盖在 Tab 之上）
```

IssueSettingsSheet（“签发设置”）：自动签发开关 + 提前天数（1–365，默认 3；大于 14 时显示频繁签发警告——本机 RKP 证书只有约 12–14 天有效期）→ `rkp-mini-issuer config set auto_issue_days N --json`；
开启时以 `setsid` 启动 `auto-loop.sh`（pid 锁保证重复启动无害）。“上次自动检查”直接读
`tmp/auto-issue.last.{at,code,json,err}`（`code` 最后写入，缺失视为没有已完成的检查）。
危险区“删除本地证书”经 MiuixDialog 确认后执行 `rkp-mini-issuer remove --yes --json`（签发进行中或本地无证书时禁用）。

同步页进入时只显示缓存（`status --json`，含上次 `remote_check`），不联网；只有点右上角状态才联网检查。
三张状态卡的右上角共用 `shared/components/CardStateButton.vue`（状态文字 + 刷新图标，忙时转圈）。
`MiuixCard` 开启按压反馈时会在 pointerdown 里 `setPointerCapture`，之后的 click 全部落到卡片本身：卡片内的按钮
必须 `@pointerdown.stop`，否则点不到且会触发卡片点击。
SenderSyncSheet：自动推送（`auto_push` 开关 + 最小剩余有效期）、云端存储（含 Gist token / WebDAV 凭据入口）、
加密（“密码”，不提 age、不显示是否已设置）、其他（标签）、保存、危险区（红色；删除云端文件 / 重置加密密码 /
清除本地数据 / 完全重置同步设置，均先 MiuixDialog 确认）。密钥缺失或权限错误只在状态卡警告条提示。

数据与后台任务（`useIssueRunner`、`useSyncRunner`、状态读取）都留在 `App.vue`，切换 Tab 不重新读取。
日志统一在 LogsSheet（“更多 → 日志”，每次打开重新读取）：签发日志 `logs/issue-*.log`、后台日志 `logs/auto.log`
（auto-loop.sh：自动签发 + 自动推送）、页面内存调试日志；同步设置弹层不再显示日志。
结果弹层的“查看签发日志”直接打开 LogsSheet 并展开最新签发日志。

### 7.2 组件映射（`miuix-vue` 已核对的导出名）

| 用途 | 组件 |
|---|---|
| 顶部栏 | `MiuixTopAppBar` |
| 分组容器 | `MiuixCard`、`MiuixSmallTitle` |
| 文本 | `MiuixText`（`type`: `title4` / `body1` / `body2` / `footnote1`；`color` 可传 CSS 变量） |
| 主/次按钮 | `MiuixButton`（`type="primary"` / 默认）、`MiuixIconButton` |
| 列表行 | `MiuixBasicComponent`（`title` / `summary` / `summaryColor` / `clickable`） |
| 开关/勾选 | `MiuixSwitch`、`MiuixCheckbox` |
| 弹层 | `MiuixBottomSheet`、`MiuixDialog`（`v-model` + `title`/`summary`） |
| 反馈 | `MiuixSnackbarHost` + `showSnackbar`、`MiuixProgressIndicator` |
| 分割线/滚动 | `MiuixDivider`、`MiuixScrollArea` |
| 图标 | `MiuixIcon` + 具名图标（`miuix-vue/icons` 或 `IconCheck` 等内置） |

**没有 `Badge`**：状态胶囊自己写（`border-radius: 9999px`，`tertiaryContainer` 底 + `onTertiaryContainer` 字），约 10 行 CSS。

组件 props 以 `node_modules/miuix-vue/dist/src/**/*.d.ts` 为准，不要按记忆写。

### 7.3 状态映射

| 条件 | 胶囊 | 颜色 |
|---|---|---|
| `keybox.exists && keybox.valid && days_left > 3` | 有效 | `tertiaryContainer` 底 / `onTertiaryContainer` 字 |
| `valid && days_left <= 3`（`EXPIRING_DAYS`；本机 RKP 证书仅约 12–14 天） | 即将过期 | 应用层 `--app-warning`（Miuix 无 warning 令牌） |
| `days_left < 0`（或 `not_after <= now`） | 已过期 | `error` |
| `exists && !valid` | 无效（附 `keybox.error`） | `error` |
| `!exists` | 未签发 | `onSurfaceVariantSummary` |

按表中顺序判断，先匹配先生效（`not_after <= now` 优先于 `valid`）。

主按钮禁用条件：`blockers.length > 0` 或任务运行中。禁用时在按钮上方显示第一条 blocker 的中文提示。

### 7.4 令牌子集（来自 `miuix-vue/style.css`，不要重复定义）

| 变量 | Light | Dark |
|---|---|---|
| `--m-color-primary` | `#3482ff` | `#277af7` |
| `--m-color-background` | `#ffffff` | `#242424` |
| `--m-color-on-background` | `#000000` | `#e6ffffff` |
| `--m-color-surface-container` | `#ffffff` | `#242424` |
| `--m-color-secondary-variant` | `#f0f0f0` | `#434343` |
| `--m-color-on-surface-variant-summary` | `#99000000` | `#80ffffff` |
| `--m-color-on-surface-variant-actions` | `#66000000` | `#66ffffff` |
| `--m-color-outline` | `#d9d9d9` | `#404040` |
| `--m-color-divider-line` | `#e0e0e0` | `#393939` |
| `--m-color-error` | `#e94634` | `#f12522` |
| `--m-color-tertiary-container` | `#eaf2ff` | `#2b3b54` |
| `--m-color-on-tertiary-container` | `#3482ff` | `#4788ff` |
| `--m-color-window-dimming` | `rgba(0,0,0,.3)` | `rgba(0,0,0,.6)` |
| `--m-radius-md` | `16px` | 同 |
| `--m-text-title4-size` | `18px` | 同 |
| `--m-text-body1-size` | `16px` | 同 |
| `--m-text-footnote1-size` | `13px` | 同 |

应用层新增：`--app-warning: #f5a623`（light）/ `#ffb74d`（dark），用于“即将过期”。

### 7.5 交互细节

- 主题：启动时 `setThemeMode(localStorage['rkp.theme'] ?? 'system')`；库**不持久化**，自己存 `system|light|dark`。
- 复制：`navigator.clipboard.writeText`，失败回退隐藏 textarea + `execCommand('copy')`；成功后 `showSnackbar`。
- 触控反馈由库的 `::after` alpha 叠加层提供；不要额外加 `hover` 样式。
- 弹层必须有自己的关闭按钮与遮罩点击关闭；不要依赖系统返回键。
- 运行中离开页面前给出提示文案；重新进入时按 4.4 恢复。
- 所有长文本（`head_hex`、日志）放可折叠区域，默认收起。

---

## 8. bridge 封装

`src/lib/ksu.ts`：

```ts
import { exec as ksuExec } from 'kernelsu'
import { mockExec } from '../dev/mockKsu'

export interface ExecResult { errno: number; stdout: string; stderr: string }

const TIMEOUT_MS = 10_000

export async function exec(command: string): Promise<ExecResult> {
  if (import.meta.env.DEV && !('ksu' in window)) return mockExec(command)
  const timeout = new Promise<never>((_, reject) =>
    setTimeout(() => reject(new Error(`exec timeout: ${command}`)), TIMEOUT_MS),
  )
  return Promise.race([ksuExec(command), timeout])
}
```

要点：

- `kernelsu@3.3.0` 是 ESM 语法但包里没有 `"type": "module"`，**只能在打包器里用**（Vite/esbuild 正常），不要在 Node 里直接 import。
- 包在模块加载时用 `typeof ksu !== "undefined"` 判空，因此在普通浏览器里 import 不会抛错，`exec` 会 reject；dev mock 必须在调用前短路。
- `errno` 即进程退出码。
- 超时只是放弃等待，**不会**杀掉后台命令；对签发不适用（签发走 runner）。

---

## 9. 安全与凭据

- 永不读取、打印、传输、渲染 `keybox.xml`、`key.pem`、CSR 全文。
- 只使用 `status.json` 与 `issue` 信封中的字段；`keybox.path` 仅作为字符串展示。
- 不提供导出/下载/分享。
- 日志可能包含路径与错误信息，可以展示；仍避免整份 CSR。
- 不在页面里拼接用户输入到 shell 命令；所有命令路径为常量。
- 不引远程资源（CSP + 离线要求）。

---

## 10. 分阶段实施计划

### Phase 0 — Spike（先做，决定后续）

1. `webui/` 最小页：`MiuixButton` + 一个按钮调用 `exec('id -u')`，把结果显示在页面。
2. 构建后推到设备：
   `adb push webui/dist/. /sdcard/webroot/ && adb shell su -c 'cp -R /sdcard/webroot/. /data/adb/modules/rkp_mini/webroot/'`
3. KernelSU 模块卡片出现“打开”，页面正常渲染、深色跟随系统、`id -u` 返回 `0`。

**验收**：页面能起、`ksu.exec` 可用、中文正常、无远程请求。

### Phase 1 — 骨架与设计系统

`package.json` / `vite.config.ts` / `tsconfig.json` / 主题 / 状态胶囊 / `StatusCard` / `CheckCard`，全部用 fixture 驱动。

**验收**：`bun run dev` 在浏览器里覆盖 5 种状态；`bun run build` 通过 `vue-tsc`。

### Phase 2 — 状态读取与刷新

`src/lib/rkp.ts`：`readStatus()`、`refreshStatus()`；页面挂载时读取，下拉/按钮刷新；错误态（读失败、JSON 解析失败、非 0 退出）各有独立 UI。

**验收**：设备上状态与 `su -c 'rkp-mini status'` 一致；blocker 显示正确。

### Phase 3 — 签发流程

`runner/issue.sh`、`runner.ts`（启动、轮询、恢复）、确认弹层、进度、结果弹层、失败诊断。

**验收**：
- 勾选前按钮禁用；勾选后启动，页面显示“进行中”。
- 退出 WebUI 再进入，仍能看到进行中/已完成。
- 成功：状态卡更新，`module.prop` 描述更新。
- 失败（拔网、断 TA）：显示 stage/code/ta_status，旧 keybox 仍在。
- 连点两次只会启动一个任务（锁生效）。

### Phase 4 — 详情与设置

证书详情弹层、指纹复制、日志列表与查看、只读 TOML、版本信息。

**验收**：复制成功；日志只读不写；不出现任何私钥内容。

### Phase 5 — 打包与设备验收

`build.sh` 集成、ZIP 校验、`AGENTS.md`/`README` 更新、真机全流程走一遍（见 §11）。

**验收**：`dist/rkp_mini-vX.Y.Z.zip` 根目录含 `webroot/index.html`；安装后 Open 与 Action 两个入口都正常。

### 可选 Phase 6 — 阶段进度

给二进制加 `--progress`：在 `issue` 的各步骤向 stderr 输出单行
`rkp-progress:{"id":"ta","state":"start"}`，页面 tail `issue.last.err` 映射成步骤条。
**不改变 stdout 契约**。v1 不做，先用“已用时 + 日志尾部”。

---

## 11. 测试与验收

### 11.1 Host

```sh
cd webui
bun install --frozen-lockfile
bun run build          # vue-tsc + vite build
bun run dev            # 浏览器 + mock ksu（fixture）
```

- 覆盖：就绪、blocked、运行中、成功、失败、结果未知、锁残留。
- 检查产物：`ls -l dist/assets`，CSS/JS 各一个文件，总 gzip < 150 kB。
- `grep -R "eval(\|new Function" dist` 应为空。

### 11.2 设备清单

| 项 | 期望 |
|---|---|
| 打开 WebUI | 页面渲染，无白屏，背景不透出 |
| 深色模式 | 跟随系统；手动切换后重开仍保持 |
| 系统字体放大 | 文本随之变大，不裁切 |
| `status --json` | 与 CLI 输出一致 |
| blocker | 主按钮禁用且原因可见 |
| 签发成功 | 状态与描述更新，剩余天数正确 |
| 签发失败 | 阶段/错误码/`ta_status` 可见，旧 keybox 未被替换 |
| 中途退出再进入 | 结果不丢 |
| 连点签发 | 只跑一个任务 |
| 返回键 | 关掉 WebUI 而不是回到上一屏（设计如此） |
| `unzip -l` | ZIP 根含 `module.prop`、`bin/rkp-mini`、`webroot/index.html` |

---

## 12. 风险与已知缺陷

| 风险 | 说明 | 处理 |
|---|---|---|
| `miuix-vue` 是 0.1.x | API 可能变 | 精确锁 0.1.1；升级前读 changelog 并跑 `vue-tsc` |
| 类型入口损坏 | 见 §2.2 | 附录 B 的 `paths`，已验证 |
| `exec` 无超时 | 卡住时无法取消 | 前端 10 s 超时；签发不走 `exec` 等待 |
| manager 被杀 | 后台任务可能中断 | `setsid` 脱离 + 文件状态；重进可见“结果未知” |
| 并发签发 | Action 按钮也能跑 `issue` | WebUI 锁只能挡 WebUI；根治需在二进制加锁（后续） |
| WebView 版本差异 | 老 manager 可能无 `ksu`/不同 origin | 启动探测 `window.ksu`，缺失显示兼容提示 |
| 文本不可选 | 无（DOM 渲染，反而更好） | — |
| 打包体积 | 新增约 90 kB gzip | 可接受，无需优化 |

---

## 13. 文档同步（实现阶段必须做）

- `AGENTS.md`：删除/改写 “No TEE/boot-state spoofing, WebUI, ...”，补充 `webui/` 目录、Bun 构建前置、`build.sh` 行为。否则下一个 Agent 会被旧约束误导。
- `README.md`：安装后如何打开 WebUI；失败时看哪里。
- `module/module.prop`：版本/versionCode 与 `Cargo.toml` 保持一致；`webui/package.json` 的 version 不参与模块版本。

---

## 附录 A：命令速查

```sh
# 主机（WebUI）
cd webui && bun install --frozen-lockfile
cd webui && bun run build          # vue-tsc + dist/issuer + dist/deployer
cd webui && bun run dev:issuer     # 签发端预览（fixture）
cd webui && bun run dev:deployer   # 部署端预览（fixture）

# Rust 侧
cargo fmt --check
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo test --workspace --locked

# 打包
NDK=/path/to/ndk ./build.sh
unzip -l dist/rkp_mini_issuer-v0.2.0.zip
unzip -l dist/rkp_mini_deployer-v0.2.0.zip

# 设备
su -c '/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-issuer status --json'
su -c '/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-issuer issue --acknowledge --json'
su -c '/data/adb/modules/rkp_mini_deployer/bin/rkp-mini-sync --role receiver --json status'
su -c '/data/adb/modules/rkp_mini_deployer/bin/rkp-mini-deployer --role receiver --json plan'

# 迭代（不重装模块）
adb push webui/dist/issuer/. /sdcard/webroot/
adb shell su -c 'cp -R /sdcard/webroot/. /data/adb/modules/rkp_mini_issuer/webroot/'
```

---

## 附录 B：类型兜底（已验证）

`webui/tsconfig.json`：

```json
{
  "compilerOptions": {
    "strict": true,
    "noEmit": true,
    "target": "es2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "skipLibCheck": true,
    "module": "esnext",
    "moduleResolution": "bundler",
    "jsx": "preserve",
    "types": ["vite/client"],
    "paths": {
      "miuix-vue": ["./node_modules/miuix-vue/dist/src/index.d.ts"],
      "miuix-vue/icons": ["./node_modules/miuix-vue/dist/src/icons/extended/index.d.ts"]
    }
  },
  "include": ["src/**/*.ts", "src/**/*.vue"]
}
```

- TypeScript 6 已移除 `baseUrl`，`paths` 直接相对 tsconfig 解析，不要再加 `baseUrl`。
- `src/env.d.ts`：

```ts
/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<{}, {}, any>
  export default component
}
```

---

## 附录 C：最小页面骨架

```vue
<script setup lang="ts">
import { onMounted, ref } from 'vue'
import {
  MiuixButton, MiuixCard, MiuixText, MiuixSmallTitle,
  MiuixSnackbarHost, setThemeMode,
} from 'miuix-vue'
import { exec } from './lib/ksu'

setThemeMode((localStorage.getItem('rkp.theme') as 'system' | 'light' | 'dark') ?? 'system')

const version = ref('…')

onMounted(async () => {
  const r = await exec('/data/adb/modules/rkp_mini/bin/rkp-mini --version')
  version.value = r.errno === 0 ? r.stdout.trim() : `读取失败 (${r.errno})`
})
</script>

<template>
  <main class="page">
    <MiuixSmallTitle text="RKP Mini" />
    <MiuixCard>
      <MiuixText type="body2">二进制版本：{{ version }}</MiuixText>
    </MiuixCard>
    <MiuixButton type="primary">签发新证书</MiuixButton>
    <MiuixSnackbarHost />
  </main>
</template>

<style>
.page { padding: 16px; display: flex; flex-direction: column; gap: 12px; }
</style>
```

---

## 附录 D：本方案的验证记录

| 项 | 结果 |
|---|---|
| `miuix-vue@0.1.1` 生产构建 | 通过，JS 240.39 kB / 83.89 kB gzip，CSS 35.78 kB / 5.72 kB gzip |
| 图标 tree-shaking | 通过（引 2 个图标未拉入 1.3 MB 图标集） |
| CSS 远程资源 | 无 `url()` / `@import` / http |
| 产物 `eval`/`new Function` | 无 |
| `tsc` 直接解析类型 | 失败 `TS7016`（上游缺陷） |
| 加 `paths` + `skipLibCheck` 后 `tsc` | 通过（exit 0） |
| Vue/TS 版本 | Vue 3.5、Vite 8.3.7、TS 6.0、Bun 1.3.14、Node 26 |
| 设备端运行 | **未验证**，Phase 0 必须先做 |
