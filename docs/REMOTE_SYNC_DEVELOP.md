# RKP Mini 远程同步（rkp-mini-sync）设计方案

> 状态：CLI/协议已实现（2026-11-10）；**§19 WebUI 修订**为同日新增决策，按 §19 实现。
> 读者：接手实现的新 Agent 会话。
> 先读：仓库根 `AGENTS.md`（Rust 侧约束与验证命令），再读本文。
> 范围：新增“加密同步 + 接收端部署”能力；不改现有 KeyMint `0x3122` 签发协议，但**扩展仓库 scope**（见 §13）。
> 本文的决策已经过产品讨论确认；实现时不要重新论证，除非发现事实性错误。

---

## 0. 一句话

把 `rkp-mini-issuer` 签发出的 `keybox.xml` 用 **age v1 口令加密**成单文件密文，上传到用户自己的 **GitHub Gist / WebDAV / 公开 HTTPS**；接收端通过 **不含密码的 URL Scheme** 导入位置，手动输入密码解密、校验、写入本地 `sync-state`，再由**独立部署层**决定写入 OhMyKeymint、TEESimulator 或其它目标。

- 没有服务端。
- 没有账号注册、设备注册、远端撤销列表。
- 云端只看到密文。
- URL/二维码永远不包含 age 密码、Gist token、WebDAV 密码。
- 同步层与部署层解耦；`sync-state` 不出现 target/path。
- 独立二进制 `rkp-mini-sync`，不把网络/密码/同步依赖塞进 `rkp-mini-issuer`。

---

## 1. 已定决策（不要再重新论证）

| 决策 | 结论 | 理由 |
|---|---|---|
| 服务端 | **完全不做** | 使用现有 Gist/WebDAV/公开 HTTP；避免部署、账号、撤销服务 |
| 加密 | age v1 passphrase + ASCII armor | 成熟、口令模式简单、Gist 需要文本 |
| manifest | 放在 age 密文内部（CBOR） | 不需要额外签名公钥；密码同时提供机密性和完整性 |
| 远端文件 | 一个 `rkp-sync.age` | 原子替换简单；无多文件事务 |
| Gist 接收端 | **直链，不需要 GitHub Token** | secret gist 的 raw URL 可直接读；密码保护内容 |
| Gist 发送端 | 需要 `gist` scope token | 创建/更新 gist 必须认证 |
| WebDAV | 自定义路径/文件名；密码单独输入 | 兼容坚果云等；URL 不含密码 |
| 公开 HTTP | 支持 `provider=url` | 部分 WebDAV 有公开分享链接；仍需密码 |
| URL Scheme | 只含位置和 stream 元数据 | 密码/token 永不进 URL/二维码 |
| 状态 | 拆成 `sync-state` 与 `deploy-state` | 同步层不知道部署目标，便于解耦 |
| `sync-state` | 不含 `target` / `target_path` / owner / reload | 部署方式在接收后由部署层决定 |
| 二进制 | 独立 `rkp-mini-sync` | 接收端不需要 KeyMint/TA；降低攻击面和依赖 |
| 自动运行 | 默认 `secret-file` | 发送端/接收端都能长期无人值守 |
| 接收端设备 | **新设备**，跨设备使用 | 必须在 UI 首次启用时接受风险 |
| 默认开关 | `sync.enabled = false` | 远程私钥下发是高危能力 |
| 身份轮换 | 默认 `scheduled` | 避免每次 push 都换 keybox；临近到期才自动轮换 |
| 管理界面 | **WebUI 为主入口**（发送端并入 `rkp_mini_issuer` WebUI；接收端模块自带 WebUI），CLI 保留同等能力 | 见 §19 |
| 密钥写入 | WebUI 经 `ksu.exec` 用 shell 内建把 **hex 编码**的密钥写入 0600 inbox 文件，`rkp-mini-sync` 读取、校验、落盘并删除 inbox | `ksu.exec` 走 root shell 的 stdin，不进 argv；见 §19.2 |
| 非交互确认 | `enable --accept-risk <版本>`；`fetch --accept-first <sha>`；`rkp-mini-deployer apply --confirm-first/--allow-identity-change <sha>` | 确认绑定到用户看到的那个版本，防并发替换 |
| 自动签发 | 以 `auto_issue_days` 为“剩余天数阈值” | 不做无条件周期签发 |
| 工具配置 | **不同步** | OMK `[crypto]`、TEESim `config.json` 是设备/身份相关 |
| 额外签名 | 默认不做 Ed25519 | 密码是唯一信任锚；减少公钥分发 |
| 撤销 | 换密码 + 重新 issue keybox | 无服务端，无法真正远程撤销旧 keybox |

---

## 2. 目标与非目标

### 2.1 目标

1. 发送端能把当前 `keybox.xml` 加密上传到 Gist / WebDAV / 公开 HTTPS。
2. 接收端能通过 URL Scheme 导入远端位置，输入 age 密码，下载、解密、校验。
3. 接收端能独立决定把 verified artifact 部署到哪个工具、哪个路径、什么权限。
4. 支持长期自动运行：发送端定时检查/签发/推送，接收端定时拉取/部署。
5. 支持身份轮换策略，避免“自动更新”意外删除 OMK 的 key。
6. 所有凭据（age 密码、Gist token、WebDAV 密码、keybox 私钥）不出现在 URL、二维码、日志、WebUI 文本中。
7. 首次启用远程私钥下发时，用户必须明确接受风险；之后可在设置中禁用。

### 2.2 非目标

- 不做 Worker/R2/D1、账号体系、设备注册、远端撤销列表。
- 不做多写者并发控制；一个 stream 只允许一个发送端。
- 不做 keybox 内容编辑、证书链修改、密钥重用。
- 不做 OMK `config.toml` / `injector.toml`、TEESim `config.json` 的同步。
- 不做任意远端命令/路径下发。
- 不承诺 Play Integrity 通过；跨设备使用不承诺合法 RKP provisioning。
- 不做 Ed25519 manifest 签名（保留为未来硬化选项）。
- 不做自动回滚到旧 seq；回滚必须人工重新发布更高 seq。
- 不做服务端式的“设备撤销”；撤销 = 换密码 + 重新 issue。

---

## 3. 术语

| 术语 | 含义 |
|---|---|
| sender | 运行 `rkp-mini-issuer` 的签发设备 |
| receiver | 新设备上的接收端模块/程序 |
| stream | 一条同步链路；一个 `stream_id` + 一个 age 密码 |
| provider | `gist` / `webdav` / `url` |
| envelope | `rkp-sync.age`，age 加密后的单文件 |
| plaintext | 解密后的 CBOR manifest + keybox |
| verified artifact | `fetch` 解密并校验后的 keybox 文件 |
| deploy target | OMK / TEESim / TrickyStore / custom |
| identity change | 新 keybox 的 `keybox_sha256` 与已部署的不同 |
| rotation | 因 RKP 重新签发导致 keybox 身份变化 |
| `auto_issue_days` | 叶子剩余天数低于该值时触发签发（阈值，不是固定周期） |

---

## 4. 已知事实（不要再重复调研）

以下事实在 2026-11-10 的会话中通过读本仓库源码、OhMyKeymint 源码和 TEESimulator 源码确认。

### 4.1 RKP-Mini

- 模块 id：`rkp_mini_issuer`；二进制：`/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-issuer`。
- 数据根：`/data/misc/keystore/rkp-mini`；默认输出：`output/`。
- 输出文件：`keybox.xml`、`cert-chain.pem`、`cert-chain.der`、`private-key.pem`、`meta.json`；凭证/数据文件 `0600`，输出目录 `0700`。
- `keybox.xml` 是 **EC-only**：一个 `algorithm="ecdsa"` 的 `<Key>`，SEC1 `EC PRIVATE KEY`，证书链 leaf→root。
- `DeviceID` 格式：`rkp-mini-` + `sha256(public key SEC1)[:12]`（`src/rkp/keybox.rs`）。
- 每次 `issue` 生成新的 P-256 key；失败保留旧 `keybox.xml`。
- `meta.json` 含 `issued_at`、`leaf_not_after`、`device_id`、`chain_sha256` 等公开诊断字段；不含私钥。
- `auto_issue_days` 当前是保留字段（默认 0 = 关闭），尚未实现调度。
- 当前 scope 明确排除 “remote private-key deployment”；本方案要求修改该 scope（§13）。

### 4.2 OhMyKeymint（OMK）

以下 §4.2–§4.3 于 2026-11-10 按上游源码复核（OMK `9069c6c`/1.2.6、TEESimulator `dev@06f9f2e`/canary-76、TEESimulator-RS `6d241e5`/v6.0.1、TrickyStore `master@3a515c5` 与 1.4.1 发布包）；均为读源码结论，未在真机验证。

- 模块 id：`oh_my_keymint`（`/data/adb/modules/oh_my_keymint`）。
- 活动 keybox：`/data/misc/keystore/omk/keybox.xml`，编译期常量，不可配置。
- 权限：`0600`，owner `1017:1017`（keystore uid）；由 `post-fs-data.sh` 与 keymint 启动时设置。进程启动后降权到 1017，替换文件若是 `root:root 0600` 会读取失败（保留旧 keybox）。
- 热重载：inotify 监听父目录，mask `IN_CLOSE_WRITE | IN_CREATE | IN_MOVED_TO`，只处理名字等于 `keybox.xml` 的事件 → 同目录临时文件 + `rename(2)` 会立即重载，临时文件自身的事件被忽略。**不要原地写** `keybox.xml`：`IN_CREATE` 时文件为空，解析失败后 OMK 会用内置模板覆盖它。
- 重启开关：`/data/adb/omk/restart.keymint`（或 `restart.all`）由 wrapper 每 2 秒轮询是否存在，消费后删除并重启 keymint 进程；换 keybox 不需要它。
- 解析是正则扫描而非 XML DOM：要求字面量 `<Key algorithm="ecdsa">`（或 `ec`）、`<PrivateKey>`、`<NumberOfCertificates>` 且数量与 `<Certificate>` 一致；EC 必需、RSA 可选。解析失败会用内置模板覆盖文件。`rkp-mini-issuer` 的输出满足这些要求；本仓库的校验把 `NumberOfCertificates` 视为可选，手工构造的 keybox 可能通过校验却被 OMK 拒绝。
- `config.toml` 的 `[crypto]` 是设备唯一 secret；`injector.toml` 是设备/应用范围配置。**两者都不同步**。
- `retire_stale_keybox_bound_keys` 默认 `true`：换 keybox 会删除旧 keybox 绑定的客户端 key。自动轮换必须警告。

### 4.3 TEESimulator / TEESimulator-RS / TrickyStore

三者是不同项目，注意区分：

| 项目 | 模块 id | keybox | watcher（目录级 inotify） |
|---|---|---|---|
| TEESimulator（JingMatrix，`dev`） | `teesim` | `/data/adb/teesim/<profile.keybox>`，默认 `keybox.xml` | `CLOSE_WRITE \| MOVED_TO \| DELETE`；只处理 `config.json`、`overrides.json` 与 `*.xml` |
| TEESimulator-RS（Enginex0） | `tricky_store` | `/data/adb/tricky_store/keybox.xml`（`target.txt` 的 `[name.xml]` 段可按包指定其它文件） | `CLOSE_WRITE \| MOVED_TO \| DELETE`；`*.xml` 触发缓存失效，下次使用时重读 |
| TrickyStore（5ec1cff） | `tricky_store` | `/data/adb/tricky_store/keybox.xml`，不可配置 | `CLOSE_WRITE \| DELETE \| MOVED_FROM \| MOVED_TO`；只处理名字等于 `keybox.xml` |

共同点：读取进程是 root，文件 `0:0 / 0600` 可用；同目录临时文件 + `rename(2)` 产生的 `IN_MOVED_TO` 会触发重载，无需重启。临时文件名**不得以 `.xml` 结尾**（否则 TEESimulator 会对半成品触发一次重载）；`.<name>.tmp.<pid>` 满足。

TEESimulator（`teesim`）：

- 数据目录 `/data/adb/teesim` 为 `0700`；`/data/adb/tricky_store` 只在安装时用于一次性拷贝旧 keybox，运行期不读。
- `config.json` 的 `profiles.<id>.keybox` 是相对文件名，例如 `"keybox": "keybox.xml"`；文件必须直接位于 `/data/adb/teesim` 下，且文件名以 `.xml` 结尾才会被 watcher 重载。
- 重载后对已有 key 做 re-attest（`ReAttest`），key blob 不变。
- `rust/teesim-km/src/attest.rs` 明确支持 EC-only；每个 `<Key>` 链至少 2 张证书。README 写“RSA + ECDSA”，与源码不一致；以目标版本实测为准。
- 若某个 profile 的 keybox 解析失败，提交后的 profile 表可能为空，所有目标暂时失去 TA。
- WebUI 导入 keybox 时写 `0644`，但目录 `0700`；接收端写 `0600` 由 root 读取也可行。
- `config.json` 包含 profile、应用范围、设备身份等，**不同步**。

TEESimulator-RS 与 TrickyStore（`tricky_store`，同一模块 id 与路径，二者互斥安装，共用 `tricky_store` 目标）：

- 都接受 EC-only；TrickyStore 1.1.0 要求 `NumberOfCertificates` 与实际数量一致，1.4.1 不读该字段。
- 不删除客户端 key。TEESimulator-RS 只清空 patched chain 缓存：替换前已生成/已缓存的 key 继续返回旧证书链，直到守护进程重启；TrickyStore 开源版同样不清理已生成 key 的缓存。
- 解析失败不会回退到旧 keybox（表现为没有可用的 attestation key）。

缺 SEC1 `parameters`（namedCurve）的私钥——即本次改动前 `rkp-mini-issuer` 的输出——在各目标的处理（仍为读源码结论，未在真机验证）：

- TrickyStore（开源 `3a515c5` + BouncyCastle 1.78.1，以及 1.4.1 dex）**拒绝**：`PEMParser` 得到 `PEMKeyPair`，其 AlgorithmIdentifier 参数为 null，`JcaPEMKeyConverter.getKeyPair` 随即失败；异常发生在 `keyboxes.clear()` 之后，整份 keybox 被丢弃，完全不做 attestation patch。
- TEESimulator-RS（`6d241e5` + BouncyCastle 1.83）**拒绝**：同一个 `JcaPEMKeyConverter` 失败（"EC domain parameters must be encoded in the algorithm identifier"）；只丢弃该 `<Key>`，EC-only keybox 因此没有可用 attestation key（-75）。
- TEESimulator（JingMatrix `06f9f2e`）**接受**：私钥只是不透明 DER，内置 BoringSSL 补丁（`rust/patches/kmr-crypto-boring-ec-group.patch`）用调用方提供的 group 解析 SEC1，`parameters` 可选。

因此旧 keybox 只在 TEESimulator 上可用；要在 OhMyKeymint / TrickyStore / TEESimulator-RS 上工作必须重新签发。

### 4.4 Gist / WebDAV

- GitHub Gist 的 “secret” 不是 private：任何拿到 URL 的人都能读；密文仍依赖 age 密码。
- Secret gist 的 raw URL 可无 token 读取；匿名 API 有速率限制（约 60 次/小时）。
- Gist 更新用 `PATCH /gists/{id}`，revision 原子提交；历史保留旧密文。
- Gist 直链应使用“最新版本”URL，不要使用带 revision 的 URL。
- WebDAV（如坚果云）使用应用密码；ETag/If-Match/MOVE 支持因提供商而异。

---

## 5. 架构与信任边界

### 5.1 分层

```mermaid
flowchart LR
  A[rkp-mini<br/>签发 keybox] --> B[rkp-mini-sync push<br/>CBOR + age + 上传]
  B --> C[(Gist / WebDAV / 公开 HTTPS)]
  C --> D[rkp-mini-sync fetch<br/>下载 + 解密 + 校验]
  D --> E[verified artifact<br/>staging 0600]
  E --> F[deploy 层<br/>接收端本地决策]
  F --> G[OhMyKeymint]
  F --> H[TEESimulator]
  F --> I[TrickyStore / custom]
```

| 层 | 职责 | 明确不知道 |
|---|---|---|
| issuer | 产生 keybox | 接收端部署目标 |
| sync | 远端寻址、加解密、seq/防回放、下载、校验、staging | target/path/owner/reload |
| deploy | 目标选择、兼容性检查、权限、原子写、reload、部署状态 | provider、密码、URL、seq |
| UI | 展示状态、风险确认、输入密码/位置 | 不展示 keybox/私钥/密码 |

### 5.2 信任边界

| 对象 | 信任程度 | 说明 |
|---|---|---|
| age 密码 | **完全信任** | 持有者可解密、可伪造新密文；必须强口令、每 stream 唯一 |
| Gist / WebDAV / HTTP | 不可信（机密性/完整性），只信可用性 | 可读、可改、可回放、可删除 |
| URL/二维码 | 不可信 | 只作位置提示；错误内容会解密失败或被 state 拒绝 |
| `sync-state` / `deploy-state` | 本地信任 | 防回放/部署记录；丢失进入 TOFU |
| 本地适配器配置 | 本地信任 | 唯一决定写哪个文件、权限、reload |
| keybox | 高敏感凭据 | 私钥；绝不进 URL/日志/WebUI/云端明文 |

### 5.3 威胁模型

| 威胁 | 后果 | 控制 |
|---|---|---|
| 云读取密文 | 私钥泄露 | age 口令加密；云端无明文 |
| 云篡改密文 | 植入恶意 keybox | age AEAD 认证；解密失败即拒绝 |
| 云回放旧密文 | 降级/过期 | `seq` + `prev` + 本地 `sync-state` + 叶子到期 |
| 云删除/扣留 | 停止更新 | 保留当前 keybox；状态告警；下轮重试 |
| Gist token 泄露 | 写坏 gist | token 最小 scope；发送端单写者；回读校验；撤销 token |
| WebDAV 密码泄露 | 写坏文件 | 应用密码可撤销；临时文件 + MOVE；If-Match |
| URL 钓鱼 | 指向攻击者存储 | 没有密码解不开；接收端展示位置并要求确认 |
| 密码泄露 | 完全失守 | 强口令、secret-file `0600`、轮换密码 + 重新 issue |
| 跨设备 keybox | 冒充/完整性失败 | 首次启用风险确认；文档明确不是合法 RKP provisioning |
| 本地 root 被攻破 | 密码/keybox 泄露 | 无法在 root 威胁下保护；轮换 + 重新 issue |
| state 丢失 | TOFU 回放窗口 | 首次导入人工核对 seq/时间/device_id/到期 |
| OMK 自动轮换 | 删除旧 key | identity gate + `retire_stale_keybox_bound_keys` 警告 |
| SSRF / XXE | 本地服务或解析器被攻击 | 限制 provider/URL、禁 DTD/实体、大小限制 |
| 自定义 scheme 劫持 | 钓鱼接收端 | 优先在接收端内粘贴/扫码；URL 不含密码 |

---

## 6. 密文格式

### 6.1 远端文件

```text
rkp-sync.age
```

- 内容：age v1 **ASCII armor** 文本。
- 加密模式：age passphrase（scrypt + ChaCha20-Poly1305）。
- 文件名固定；Gist 一个 stream 一个 gist；WebDAV 允许自定义文件名，但建议稳定。
- 不做压缩（keybox 很小，避免额外解析面）。

### 6.2 明文 CBOR

```text
{
  v: 1,                         // uint
  stream_id: "st_01J...",       // text
  seq: 42,                      // uint64, >= 1
  prev: h'...' | null,          // 上一版 plaintext 的 SHA-256；第一版 null
  issued_at: 1762750000,        // int64 Unix 秒
  keybox: {
    sha256: h'...',             // keybox_xml 的 SHA-256
    device_id: "rkp-mini-...",  // text
    leaf_not_before: 1762700000,
    leaf_not_after: 1794000000,
    chain_sha256: h'...'
  },
  keybox_xml: h'...',           // 原始 XML 字节
  meta_json: h'...' | null      // 可选，诊断用；不参与安全决策
}
```

编码规则（保证不同实现可复现）：

- 使用 CBOR definite-length；禁止 indefinite-length。
- map key 为 text；字段顺序固定为上面列出的顺序。
- 不使用 tags、不包含浮点数。
- 字符串 UTF-8；字节串用 bstr。
- `seq` 从 1 开始；发送端不能回退。
- `prev` = 上一版**明文 CBOR 原始字节**的 SHA-256；第一版为 null。
- `plaintext_sha256` 不入 CBOR，由接收端对解密后的原始字节计算。
- `keybox.sha256` = `keybox_xml` 原始字节的 SHA-256。
- `meta_json` 只作诊断；接收端不得依据其做安全判断。

### 6.3 大小限制

| 对象 | 上限 |
|---|---|
| 整个 age 文件 | 256 KiB |
| 解密后的 CBOR | 192 KiB |
| `keybox_xml` | 128 KiB |
| `meta_json` | 16 KiB |
| URL 解码后的 JSON | 4 KiB |
| Gist/WebDAV 单个响应 | 256 KiB |

超限直接拒绝；不要边读边无限缓冲。

### 6.4 口令规则

- UTF-8；只去掉输入行尾的单个 `\n` 或 `\r\n`；**不 trim 首尾空格**。
- 建议至少 6 个随机单词或 20+ 随机字符；每个 stream 唯一。
- 不提供“密码校验字段”；解密失败统一报 `decrypt_failed`。
- 不在日志、错误消息、URL、二维码、外部进程 argv、环境变量中出现密码；WebUI 的写入路径见 §19.2（经 shell stdin + 内建命令，不产生含密码的 argv）。
- `secret-file` 内容为原始口令；读取时只去掉行尾换行；文件 `0600`。
- 默认使用 age 的 scrypt 参数；若低端设备耗时明显，记录耗时但不要降低到不可接受的安全参数。

### 6.5 为什么不做 Ed25519

- manifest 在密文内，age AEAD 已经认证；
- 额外签名需要分发/固定签发公钥，和“无服务端、轻量”冲突；
- 密码泄露时签名也救不了；
- 保留为未来硬化选项，不属于 v1。

---

## 7. 状态

### 7.1 `sync-state.json`

只描述“同步层验证到哪一版”，**不包含 target/path**。

```json
{
  "v": 1,
  "provider": "gist",
  "location": "https://gist.githubusercontent.com/<owner>/<id>/raw/rkp-sync.age",
  "stream_id": "st_01J...",
  "verified_seq": 42,
  "plaintext_sha256": "...",
  "keybox_sha256": "...",
  "device_id": "rkp-mini-...",
  "keybox_not_after": 1794000000,
  "last_remote_token": "etag-or-updated_at",
  "last_checked_at": 1762750000,
  "verified_at": 1762750000,
  "last_error": null
}
```

- `verified_seq` 是当前已解密并通过校验的最高 seq，不是“已部署”的 seq。
- 发送端复用同一结构记录“最近一次成功 push”：`verified_seq` = 已上传 seq，`plaintext_sha256` = 该版明文 hash；用于远端删除/重建后的 seq 延续。
- 部署层通过 `deploy-state` 记录实际部署情况。
- `location` 可随 Gist 重建/URL 更换更新，但 `stream_id` 不变；更新前必须成功验证同一 stream。

### 7.2 `deploy-state.json`

由接收端部署层维护：

```json
{
  "v": 1,
  "stream_id": "st_01J...",
  "last_seen_keybox_sha256": "...",
  "last_seen_seq": 42,
  "deployed_keybox_sha256": "...",
  "deployed_seq": 41,
  "deployed_at": 1762750000,
  "targets": [
    {
      "id": "omk",
      "path": "/data/misc/keystore/omk/keybox.xml",
      "owner": "1017:1017",
      "mode": "0600",
      "deployed_sha256": "...",
      "deployed_at": 1762750000,
      "reload": "auto",
      "reload_status": "ok",
      "last_error": null
    }
  ],
  "pending": null
}
```

- `pending` 可用于记录“已 fetch 但因 identity gate 未部署”的 `{seq, keybox_sha256}`。
- 一个 keybox 可对应多个 target；每个 target 独立记录成功/失败。
- 不把 deploy 信息写回 `sync-state`。

### 7.3 verified artifact

- `rkp-mini-sync fetch` 解密并校验后写：
  - 接收端数据目录 `staging/<keybox_sha256>.xml`，`0600`。
  - `staging/latest.json`，`0600`，记录 `{seq, keybox_sha256, plaintext_sha256, device_id, leaf_not_after}`。
- `fetch --json` 输出 staging 路径与元数据。
- `rkp-mini-deployer apply` 读取 staging，按本地 target 配置部署；成功后清理 staging（或保留最近 1 个用于诊断）。
- staging 是明文私钥文件；目录 `0700`，文件 `0600`，不得进入备份/日志/WebUI。

### 7.4 防回放规则

接收端处理顺序：

1. 解密 → 解析 CBOR → 校验 schema/大小/stream_id。
2. `seq > sync-state.verified_seq` → 继续。
3. `seq == verified_seq`：
   - `plaintext_sha256` 相同 → 幂等，允许重新 emit artifact（供部署重试）；
   - 不同 → `seq_conflict`，拒绝。
4. `seq < verified_seq` → `sync_rollback`，拒绝。
5. `seq == verified_seq + 1` 时要求 `prev == sync-state.plaintext_sha256`。
6. 跳号（离线多版）允许，但记录 `gap=true` 并告警；不要要求下载全部中间版本。
7. `issued_at <= now + 600`；本地时钟异常（< 2020）→ `clock_invalid`。
8. 叶子 `not_after >= now + min_validity_days`；否则 `keybox_expiring`。
9. 首次导入（无 state）：TOFU，必须人工确认，展示 seq/时间/device_id/到期/目标影响。
10. state 丢失：同样 TOFU；这是无服务端方案的残余回放窗口。

### 7.5 远端删除/重建后的 seq

- 发送端本地保留 `verified_seq` 与 `plaintext_sha256`（最近一次成功 push 的值）。
- 选择基点：`base = (remote.seq >= local.verified_seq) ? remote : local`；远端不存在时用 local。
- 新 seq = `base.seq + 1`，不是固定 1。
- `prev` = `base.plaintext_sha256`；完全没有历史时才是 null。
- 远端存在但解密失败：终止，不覆盖；不要假设“远端坏了就重来”。
- 更换 Gist/WebDAV 文件后，接收端更新 `location`，但保留 `verified_seq`；新位置 seq 更低会被拒绝。
- 发送端 state 丢失且远端也丢失时，可能从 1 重新开始；接收端会拒绝低 seq，需要人工确认新 stream 或导入新的 stream_id。

---

## 8. 远端 provider

### 8.1 GitHub Gist

**最佳实践：**

| 项 | 结论 |
|---|---|
| 数量 | 一个 stream 一个 gist；不要一个 gist 放多条 stream |
| Gist ID | 使用 GitHub 生成的 ID；不要尝试自定义 |
| 文件名 | 固定 `rkp-sync.age`；更新保持同名 |
| 类型 | `public: false`（secret）；secret ≠ private |
| 描述 | 通用描述，不含设备/型号/stream 信息 |
| 发送端 | PAT 仅 `gist` scope，`0600` 存放 |
| 接收端 | 直接 GET 密文 URL，不需要 token |
| 直链 | 最新版本 URL，不带 revision |
| 更新 | `PATCH /gists/{id}` 替换文件 |
| 历史 | revision 保留旧密文，按旧密码仍可解处理 |
| 速率 | 匿名 API 约 60 次/小时；接收端轮询间隔足够 |

直链构造：

```text
https://gist.githubusercontent.com/<owner>/<gist_id>/raw/rkp-sync.age
```

不要使用：

```text
https://gist.githubusercontent.com/<owner>/<gist_id>/raw/<revision>/rkp-sync.age
```

带 revision 的 URL 会永久指向旧版本。

接收端无 token 读取：

- 直接 GET 上述 URL；secret gist 可匿名访问。
- 若响应被 CDN 缓存，可加 `Cache-Control: no-cache`；轮询间隔 6–12 小时时通常无感。
- 若直链 404 或需要更高限额，可选用 `GET https://api.github.com/gists/{id}`（仍可无 token），解析 `files["rkp-sync.age"].content`；若 `truncated`，再取 `raw_url`，并校验 host 为 `gist.githubusercontent.com`。
- 接收端不要求配置 GitHub Token；只有用户主动选择“带 token 读取”时才使用。

发送端流程：

1. 首次：`POST /gists`，`public: false`，单文件。
2. 后续：`PATCH /gists/{id}`，同名文件内容替换。
3. 回读/解析 `updated_at` 或内容 hash，确认写入。
4. 失败重试；不要并发写多个发送端。

### 8.2 WebDAV

- 允许自定义路径/文件名，例如 `/rkp/phone-a-keybox.age`。
- 发送端：
  1. `PUT` 到同目录临时文件 `rkp-sync.age.tmp`；
  2. `MOVE` 到目标，`Overwrite: T`；
  3. 不支持 MOVE 时 `PUT` 目标 + `If-Match`（若服务器支持）。
- 接收端：
  - `HEAD` 取 ETag/Last-Modified；
  - `GET` + `If-None-Match`，304 则 no-op；
  - 不支持 ETag 时直接 GET，靠解密后的 `seq/hash` 判断。
- 认证：用户名 + 应用密码；URL Scheme 可含用户名（非 secret），**不含密码**。
- 只允许 https；拒绝跨 host 重定向；拒绝 URL userinfo。
- 失败：401/403 停止并提示；409/412/423 重新读取/下轮重试；507 配额不足告警。
- 文件名可自定义，但建议稳定；改名会留下旧密文，接收端需要重新导入位置。

### 8.3 公开 HTTPS 文件

新增 `provider = "url"`：

- 直接 GET 一个公开 HTTPS 文件；无认证。
- 适用于 WebDAV 公开分享链接、静态文件服务器。
- 仍需 age 密码。
- 校验：
  - 只允许 `https://`；默认拒绝 `http://`（本地调试需显式开关）；
  - 拒绝 URL userinfo；
  - 默认拒绝 localhost/私网 IP，防 SSRF；本地调试需 `--allow-private`；
  - 限制重定向为同 host/scheme；
  - 限制响应大小；
  - 不信任 `Content-Type`，按 age armor 解析。
- 如果 URL 需要 Cookie/Basic Auth，改用 WebDAV provider，不要试图把凭据放进 URL。

### 8.4 对比

| | Gist | WebDAV | 公开 URL |
|---|---|---|---|
| 接收端认证 | 无 token（secret raw URL） | 用户名+应用密码 | 无 |
| 发送端认证 | PAT `gist` | 用户名+应用密码 | 无（只读） |
| 原子更新 | revision 原子，无 CAS | PUT+MOVE / If-Match | 不适用 |
| 自定义文件名 | 固定 | 允许 | 取决于服务器 |
| 限流 | GitHub | 提供商 | 提供商/CDN |
| 适用 | 快速、无自建 | 自托管/网盘 | 公开分享/静态站 |

---

## 9. URL Scheme

### 9.1 格式

```text
rkpsync://v1/import?p=<base64url(JSON)>
```

- `p`：无 padding 的 base64url；解码后 UTF-8 JSON。
- 整个 URI 上限 2 KiB；JSON 上限 4 KiB。
- 二维码内容就是该 URI。
- 同时支持在接收端内粘贴原始 URI/JSON；不要依赖浏览器点击自定义 scheme。

### 9.2 schema

公共字段：

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `v` | int | 是 | 固定 1 |
| `provider` | string | 是 | `gist` / `webdav` / `url` |
| `stream_id` | string | 是 | 与密文内一致；非 secret |
| `label` | string | 否 | UI 展示，≤64 字符，不含敏感信息 |
| `format` | string | 是 | 固定 `age-armor` |

Gist：

| 字段 | 必填 | 说明 |
|---|---|---|
| `url` | 推荐 | 最新版直链；接收端优先使用 |
| `gist_id` | 否 | 无 `url` 时用 API 解析（可无 token） |
| `filename` | 否 | 默认 `rkp-sync.age` |

WebDAV：

| 字段 | 必填 | 说明 |
|---|---|---|
| `endpoint` | 是 | `https://.../dav/` |
| `path` | 是 | 固定文件路径；规范化后使用 |
| `user` | 否 | 用户名；密码单独输入 |

公开 URL：

| 字段 | 必填 | 说明 |
|---|---|---|
| `url` | 是 | 公开 HTTPS 文件 |

### 9.3 示例

Gist：

```json
{
  "v": 1,
  "provider": "gist",
  "url": "https://gist.githubusercontent.com/owner/abc123/raw/rkp-sync.age",
  "gist_id": "abc123",
  "filename": "rkp-sync.age",
  "stream_id": "st_01J...",
  "label": "phone-a",
  "format": "age-armor"
}
```

WebDAV：

```json
{
  "v": 1,
  "provider": "webdav",
  "endpoint": "https://dav.jianguoyun.com/dav/",
  "path": "/rkp/phone-a-keybox.age",
  "user": "me@example.com",
  "stream_id": "st_01J...",
  "label": "phone-a",
  "format": "age-armor"
}
```

公开 URL：

```json
{
  "v": 1,
  "provider": "url",
  "url": "https://example.com/rkp/rkp-sync.age",
  "stream_id": "st_01J...",
  "label": "phone-a",
  "format": "age-armor"
}
```

### 9.4 安全校验

接收端必须：

- 拒绝任何 `password`、`passphrase`、`token`、`secret`、`key` 字段。
- 拒绝未知字段（避免走私）。
- 拒绝 URL userinfo（`https://user:pass@...`）。
- 只允许已知 provider；`endpoint`/`url` 必须 https。
- 拒绝 localhost/私网地址（除非显式 `--allow-private`）。
- 规范化 `path`，拒绝 `..`、绝对路径逃逸、控制字符。
- `filename` 只允许 `[A-Za-z0-9._-]+`，必须以 `.age` 结尾。
- `stream_id` 限定字符集/长度。
- 展示解码后的 provider/endpoint/path/stream/label，用户确认后再输入密码。
- 不在 URL 中放密码；如需传输密码，必须单独安全渠道。

### 9.5 分享与二维码

- `rkp-mini-sync share` 输出 URI + 二维码 + 可复制的 JSON。
- 分享界面把“存储位置”和“解密密码”分开显示；明确提示二维码不含密码。
- 二维码只编码 URI；不要编码密码/token。
- 自定义 scheme 可能被恶意 app 注册；优先接收端内扫码/粘贴，不要从浏览器直接点。
- `label` 只用于展示，不要放设备序列号、IMEI、邮箱等敏感信息。

---

## 10. 发送端

### 10.1 配置

发送端在 `config.toml` 增加 `[sync]`（或独立 `sync.toml`；建议复用主配置）：

```toml
[sync]
enabled = false
risk_ack_version = 0
provider = "gist"                 # gist | webdav
stream_id = "st_01J..."
min_validity_days = 14
request_timeout_seconds = 30

# Gist
gist_id = ""
gist_filename = "rkp-sync.age"
gist_url = ""                     # 可选：分享用最新版直链
gist_token_file = "/data/misc/keystore/rkp-mini/sync/gist.token"  # 0600

# WebDAV
webdav_endpoint = ""
webdav_path = "/rkp/rkp-sync.age"
webdav_user = ""
webdav_password_file = "/data/misc/keystore/rkp-mini/sync/webdav.pass" # 0600

# 口令
password_mode = "secret-file"     # secret-file | prompt
password_file = "/data/misc/keystore/rkp-mini/sync/age.pass"      # 0600
```

- `risk_ack_version` 由 UI 写入；未确认时 `enabled` 不得为 true。
- `password_mode = "secret-file"` 是默认；首次设置必须让用户确认“密码将落盘用于自动运行”。
- `gist_token_file` / `webdav_password_file` / `password_file` 都是 `0600`。
- 不把 token/密码写进 TOML 值本身；只写路径。

### 10.2 CLI

```text
rkp-mini-sync init                  # 初始化 stream/非敏感配置
rkp-mini-sync push                  # 读取 keybox，加密上传
rkp-mini-sync share                 # 输出 URL/二维码（不含密码）
rkp-mini-sync status [--json]       # 远端/seq/hash/有效期
rkp-mini-sync rotate-password       # 新密码重新加密最新 keybox
rkp-mini-sync sync-now              # push 的别名/自动任务入口
rkp-mini-sync forget                # 清除本地 token/密码/state（不删除远端）
```

`rkp-mini-sync` 不负责 `issue`；自动任务由 `rkp-mini-issuer` + `rkp-mini-sync` 编排。

### 10.3 push 流程

```text
1. 读取 keybox.xml + meta.json
2. 校验：
   - 文件存在、0600、大小限制
   - 至少一个 ecdsa Key；SEC1 私钥
   - 链 2..8 张，leaf→root 链接
   - 私钥与叶子公钥匹配
   - DeviceID == rkp-mini-<sha256(pubkey)[:12]>
   - 叶子 not_after >= now + min_validity_days
3. 读取远端 rkp-sync.age：
   - 不存在：base = local（首次 seq=0/prev=null）
   - 存在：用当前密码解密，取 seq/plaintext_sha256
   - 解密失败：终止，不覆盖远端
4. 若远端 keybox_sha256 == 本地且 seq 未变：no-op（除非 --force）
5. 选择基点：base = remote.seq >= local.verified_seq ? remote : local
6. 构造 CBOR：
   seq = base.seq + 1
   prev = base.plaintext_sha256（无任何历史则 null）
   issued_at = now
   keybox_xml = 本地内容
7. age armor 加密
8. 上传：
   Gist: PATCH /gists/{id}
   WebDAV: PUT tmp + MOVE / If-Match PUT
9. 回读校验：
   下载/解密，比较 plaintext_sha256/seq/keybox_sha256
10. 更新本地 sync-state.json；日志只记 seq/hash/provider
```

### 10.4 并发

- 发送端本地 `sync.lock`（`tmp/sync.lock` 或专用目录），WebUI/CLI/定时任务互斥。
- Gist 无 CAS：单写者；写入后回读，若内容不是自己写的 → 告警。
- WebDAV 有 `If-Match` 时使用 ETag；412 → 重新读取、seq+1、重试。
- 两个发送端同时写同一 stream 不是支持场景；需要多写者就拆 stream。

### 10.5 自动签发 + 自动推送

现有 `auto_issue_days` 从“保留字段”改为“剩余天数阈值”：

```text
每天/每 12h：
  rkp-mini-issuer issue --if-expiring-days <auto_issue_days>
  rkp-mini-sync push
```

- `days_left > auto_issue_days`：不签发。
- `days_left <= auto_issue_days`：签发新 keybox；成功后 push。
- 签发失败：保留旧 keybox，下轮重试。
- push 失败：本地 keybox 已更新，远端仍旧；下轮重试；接收端保留旧版本。
- 不做“无条件每天 issue”；那会频繁换身份，触发接收端 identity gate。
- `issue` 和 `push` 是独立步骤；`rkp-mini-issuer` 不链接 `rkp-mini-sync`。

### 10.6 发送端 guard

- 命中以下情况拒绝上传：keybox 不存在/不可读、私钥与叶子不匹配、链结构错误、叶子剩余不足、密码文件缺失/权限错误。
- `--allow-expiring` 仅用于手动应急，并在日志中记录。
- 日志绝不出现 keybox、私钥、age 密码、Gist token、WebDAV 密码。
- `share` 明确提示：链接可以公开，密码必须单独安全渠道发送。

---

## 11. 接收端

### 11.1 配置

接收端独立模块（例如 `rkp_mini_deployer`）配置：

```toml
[sync]
enabled = false
risk_ack_version = 0
provider = "gist"
stream_id = "st_01J..."
min_validity_days = 7
request_timeout_seconds = 30

# Gist（直链优先；不需要 token）
gist_url = "https://gist.githubusercontent.com/.../raw/rkp-sync.age"
gist_id = ""
gist_filename = "rkp-sync.age"

# WebDAV
webdav_endpoint = ""
webdav_path = ""
webdav_user = ""
webdav_password_file = "/data/adb/rkp_sync/secrets/webdav.pass" # 0600

# age 口令
password_mode = "secret-file"     # secret-file | prompt
password_file = "/data/adb/rkp_sync/secrets/age.pass"           # 0600

[deploy]
identity_change_policy = "scheduled"  # scheduled | always | never
rotation_lead_days = 30
auto_deploy = true

[[deploy.targets]]
id = "omk"
path = "/data/misc/keystore/omk/keybox.xml"
owner = "1017:1017"
mode = "0600"
reload = "auto"

# [[deploy.targets]]
# id = "teesim"
# path = "/data/adb/teesim/keybox.xml"
# owner = "0:0"
# mode = "0600"
# reload = "auto"
```

- `sync-state` 在 `<receiver>/sync/sync-state.json`。
- `deploy-state` 在 `<receiver>/deploy/deploy-state.json`。
- staging 在 `<receiver>/staging/`，`0700`。
- 所有 secret `0600`，目录 `0700`；不得进入模块 ZIP/云备份。

### 11.2 import / fetch

```text
rkp-mini-sync import 'rkpsync://...' # 解析位置，展示确认
rkp-mini-sync fetch [--json]         # 下载/解密/校验，写 staging
rkp-mini-deployer plan [--json]      # 只读预览（身份门控）
rkp-mini-deployer apply [--json]     # 按本地 targets 部署（--confirm-first/--allow-identity-change）
rkp-mini-sync status [--json]        # sync + deploy 状态（只读）
```

接收端的 fetch + deploy 组合由模块脚本负责（`modules/deployer/run.sh` 的
`action|run|auto`），没有顶层 `run` 子命令：`fetch` 失败仍会执行
`rkp-mini-deployer apply --if-staged`，后者自己重新校验 enabled/risk/stream。

`import`：

1. 解析 URL/QR；校验 schema/无 secret/HTTPS/路径。
2. 展示 provider/endpoint/path/stream/label；用户确认。
3. 首次启用展示风险说明（§13），写入 `risk_ack_version`。
4. 提示输入 age 密码；默认保存到 `secret-file`（用户可改 `prompt`）。
5. 可选：测试连接（只下载大小/HEAD；不要求先解密）。
6. 不自动应用 keybox；需要 `fetch` + `deploy`。

`fetch`：

1. 读取远端；条件请求（ETag/Last-Modified/API updated_at）。
2. 大小限制；age 解密。
3. CBOR schema/stream/seq/prev/时间/叶子校验。
4. 严格 keybox 校验（§11.4）。
5. 写 staging + `sync-state`；只输出同步层元数据，不读取/不比较 `deploy-state`。
6. 不修改目标文件。

`rkp-mini-deployer apply`：

1. 读取 staging/latest。
2. 读取 `deploy-state`；比较 `keybox_sha256`：
   - 与已部署相同 → 幂等，按需修复权限/reload；
   - 不同 → 走 identity gate。
3. 按 targets 逐个部署；每个 target 独立成功/失败。
4. 更新 `deploy-state`。
5. 不修改 `sync-state`。

### 11.3 严格 keybox 校验

接收端必须独立实现严格校验，不能只做字符串包含：

- 解析器禁用 DTD/外部实体（建议 `roxmltree`；拒绝 `<!DOCTYPE` / `<!ENTITY`）。
- 大小 ≤ 128 KiB。
- 恰好一个 `<AndroidAttestation>`/`<Keybox>` 结构（按 RKP-Mini 输出）。
- 至少一个 `algorithm="ecdsa"` 的 `<Key>`；拒绝仅 RSA（RKP-Mini 不产生）。
- `PrivateKey` 为 SEC1 `EC PRIVATE KEY`；解析出私钥并推导公钥。
- 证书链 2..8 张；leaf→root 顺序；issuer/subject 链接正确。
- 叶子公钥与私钥匹配。
- `DeviceID` == `rkp-mini-` + `sha256(public SEC1)[:12]`。
- 叶子 `not_before <= now + skew`；`not_after >= now + min_validity_days`。
- `keybox_sha256` 与 manifest/payload 一致。
- 证书签名/根信任/吊销检查为可选硬化；v1 至少做结构、链接、key match，并在 UI 标注“未做根信任/吊销检查”。

### 11.4 部署适配器

目标只来自本地配置；URL/密文不能指定路径/命令。

| id | 路径 | owner / mode | reload | 说明 |
|---|---|---|---|---|
| `omk` | `/data/misc/keystore/omk/keybox.xml` | `1017:1017` / `0600` | 模块 watcher（`reload = "auto"`）；`restart` 额外创建 `/data/adb/omk/restart.keymint` 重启 keymint | 模块 `oh_my_keymint`；不同步 `config.toml`/`injector.toml`；警告 `retire_stale_keybox_bound_keys` |
| `teesim` | `/data/adb/teesim/<name>.xml`，默认 `keybox.xml` | `0:0` / `0600`（目录 `0700`） | 模块 watcher | 模块 `teesim`；需匹配 `config.json` profile 的 keybox 文件名，且必须以 `.xml` 结尾；源码支持 EC-only，链 ≥2 |
| `tricky_store` | `/data/adb/tricky_store/keybox.xml` | `0:0` / `0600` | 模块 watcher | 模块 `tricky_store`：TrickyStore 或 TEESimulator-RS（同 id、同路径） |
| `custom` | 本地显式配置 | 本地显式配置 | 本地显式配置 | 必须手动 opt-in；禁止远端下发路径/命令 |

`rkp-mini-deployer targets [--json]` 只读地列出上表的内置目标，并检测对应模块是否安装（`/data/adb/modules/<模块 id>/module.prop` 存在；`disable`/`remove` 标记 → `enabled = false`；读取失败按未安装处理）。`data.builtin[]`：`id`、`path`、`owner`、`mode`、`module { id, installed, enabled, name, version }`。CLI 的 `add-target` 不受检测结果限制。

TEESim 特别说明：

- 如果 `config.json` 的 profile 引用 `foo.xml`，接收端写 `keybox.xml` 不会生效；`deploy` 应解析 `config.json`，确认 target path 的 basename 被至少一个 profile 引用；否则报 `target_unreferenced`。
- 不自动修改 `config.json`。
- README 与源码对 RSA 要求不一致；以目标版本实测为准。

OMK 特别说明：

- OMK 固定读取 `/data/misc/keystore/omk/keybox.xml`，不需要改配置。
- `retire_stale_keybox_bound_keys=true` 时换 keybox 会删除旧 keybox 绑定的客户端 key；部署前必须警告。
- `chown 1017:1017` 必须成功；失败则报错，不继续。

### 11.5 identity gate 与 rotation policy

比较 `deploy-state.deployed_keybox_sha256` 与 staging：

- 相同 → 幂等部署；可修复权限/触发 reload。
- 不同 → identity change：
  - `scheduled`（默认）：仅当旧 keybox `keybox_not_after - now <= rotation_lead_days` 或已过期，且新叶子 `not_after > 旧 not_after` 时自动部署；否则记录 `pending`，通知用户。
  - `always`：自动部署任何新身份；明确风险。
  - `never`：必须人工 `rkp-mini-deployer apply --allow-identity-change`。
- 首次部署（无 deploy-state）：必须人工确认，展示 old/new 影响；即使 `always` 也不静默。

OMK 警告必须在实际写入前展示/记录；TEESim 提示会触发已有 key 的 re-attest。

### 11.6 原子写

```text
tmp = target_dir/.<name>.tmp.<pid>
open O_CREAT|O_EXCL 0600
write + fsync
fchmod/fchown
rename(tmp, target)
fsync(target_dir)
restorecon（如需要）
读回 target，校验 sha256
写 deploy-state（原子）
```

- 任何一步失败：目标文件保持旧版本；deploy-state 记录错误。
- 不在 rename 前删除旧文件；不清空目录。
- 多 target：逐个部署；一个失败不阻塞其它，但状态分别记录。
- 不保留目标目录下的明文 `.prev`；如需回滚，由用户/部署层另行处理（v1 不提供自动回滚）。

### 11.7 自动运行与 secret-file

你已选择默认 `secret-file`，因此发送端和接收端都可长期无人值守：

- 发送端 `age.pass` 0600；接收端 `age.pass` 0600。
- 发送端每天/每 12h：`rkp-mini-issuer issue --if-expiring-days N` + `rkp-mini-sync push`。
- 接收端每 6–12h：`sh modules/deployer/run.sh auto`（`check-auto --require-verified`，
  然后 `fetch --auto` + `apply --auto --if-staged`）。
- 定时器：`service.sh` 等 `sys.boot_completed` + 网络；后台循环 `setsid/nohup` + 锁 + 抖动。
- Android Doze/厂商省电可能冻结 root shell 循环；纯 shell 不保证严格定时。若要严格定时，接收端应做成 App + WorkManager。
- `secret-file` 缺失/权限不对：自动任务停止并报错；不要静默回退到空密码或提示输入。
- 读取口令后尽快 zeroize；不把密码传给子进程 argv；必要时用 stdin/fd/inbox 文件（§19.2）。

---

## 12. 二进制拆分

### 12.1 建议

**拆成独立 `rkp-mini-sync`，建议 Cargo workspace + 共享 core。**

| 维度 | `rkp-mini-issuer` | `rkp-mini-sync` |
|---|---|---|
| 依赖 | KeyMint/vendor ABI、TA worker、RKP 协议 | age、CBOR、HTTP、URL、keybox 校验 |
| 运行位置 | 签发设备 | 发送端 + 接收端（无需 RKP 硬件） |
| 攻击面 | 高（vendor 调用） | 中（网络/密码/文件） |
| 生命周期 | 签发/状态 | 长期定时同步 |
| 测试 | 需 Android/TA | 可 host 完整测试 |
| 包体 | 保持精简 | 独立增量 |

不建议把 sync 直接塞进 `rkp-mini-issuer` 单二进制：age/网络/密码会把签发程序依赖和攻击面一起放大；接收端根本不需要 KeyMint 代码。

### 12.2 推荐布局

```text
crates/
  rkp-mini/          # 现有签发 CLI；不链接 sync 代码
  rkp-mini-sync/          # 同步/部署 CLI
  rkp-mini-sync-core/     # age、CBOR、URL、state、严格 keybox 校验
```

或者独立仓库 `rkp-mini-sync` + 发布 `rkp-mini-sync-core`。

- 发送端模块包含 `bin/rkp-mini-issuer` + `bin/rkp-mini-sync`。
- 接收端模块只包含 `bin/rkp-mini-sync` + 接收端部署适配器/WebUI。
- `rkp-mini-issuer` 不依赖 `rkp-mini-sync-core`；两者通过文件/子进程编排。
- 若不想改 workspace，可先独立仓库；不要用第二个 `src/bin` 共享全部依赖作为长期方案。

### 12.3 依赖与约束

建议保持仓库现有风格：

- 同步、阻塞 I/O；不引 async runtime、不引 `clap`、不引 `reqwest`。
- HTTP：复用 `ureq`（TLS）。
- 加密：`age`（passphrase/armor）。
- 序列化：`ciborium`、`serde`、`serde_json`。
- 哈希/编码：`sha2`、`hex`、`base64`。
- XML：`roxmltree`（纯解析、无 DTD/实体）。
- 配置：`toml`；时间：`time`。
- 错误：`anyhow`；密码内存 zeroize 可用 `zeroize`。
- release profile 与主项目一致（`panic = "abort"`；不要依赖 catch_unwind）。

### 12.4 打包

- `build.sh` 需交叉编译两个二进制（若同仓库），并检查 ZIP。
- 发送端 ZIP 根目录必须含 `bin/rkp-mini-issuer`、`bin/rkp-mini-sync`、`module.prop`、hook、`webroot/`（若 WebUI）。
- 接收端有独立 ZIP/模块；不要把接收端代码塞进 `rkp_mini_issuer`。
- `rkp-mini-sync` 有自己的 `--version` 和协议版本（`sync protocol v1`），不要与模块版本强绑定。

---

## 13. Scope、UI 与跨设备风险

### 13.1 仓库 scope 变更

本方案要求修改 `AGENTS.md` / `README.md`：

原 scope 中的：

```text
No ... remote private-key deployment.
```

改为：

```text
Remote private-key deployment is opt-in and disabled by default. It encrypts the
keybox to user-controlled storage (Gist/WebDAV/public HTTPS) with an age
passphrase. Enabling it requires explicit risk acceptance. It does not guarantee
Play Integrity, and using a keybox on a different device is keybox cloning /
spoofing, not a legitimate RKP provisioning path.
```

同时补充：

- 默认关闭；首次启用必须风险确认；可在设置中禁用。
- 密码默认落盘（`secret-file`）用于自动运行；这是明确接受的取舍。
- 不同步 OMK `config.toml`/`injector.toml`、TEESim `config.json`。
- 接收端是新设备时的跨设备风险。

### 13.2 首次启用风险确认

UI 必须展示并让用户勾选确认：

- 该 keybox 是签发设备的 RKP 身份，在新设备上使用属于 keybox 克隆/冒充。
- Play Integrity、Key Attestation、银行/支付应用可能拒绝，Google 可能撤销。
- 私钥会被复制到新设备；任一台设备泄露，另一台一起失守。
- age 密码会保存到 `0600` 文件以支持自动运行；设备丢失/root 被攻破会泄露。
- OMK 的 `retire_stale_keybox_bound_keys=true` 可能在轮换时删除旧 key。
- 这不是合法的跨设备 RKP provisioning；风险由用户承担。

写入 `risk_ack_version` 与时间；版本变化后重新确认。禁止默认勾选、禁止隐藏。

### 13.3 设置与禁用

- 设置页保留“启用/禁用同步”开关。
- **禁用**：停止定时任务、停止 fetch/deploy；保留已部署 keybox，避免突然破坏设备。
- **禁用并清除远端**：删除 `age.pass`、Gist token、WebDAV 密码、URL、sync/deploy state；可选删除 staging。
- **移除已部署 keybox**：单独操作，二次确认；可能让 OMK/TEESim 回到无 keybox 状态。
- 禁用不等于撤销：旧 keybox 仍然存在；真正撤销需换密码 + 重新 issue。

### 13.4 跨设备使用

接收端不是同一台设备，因此：

- 不承诺 RKP 合法性；这是用户主动选择的 keybox 克隆/冒充。
- 每台接收设备建议独立 stream + 独立密码；不要把一份 keybox 分发给多台设备。
- 如果未来要合法多设备，应在每台设备上分别运行 RKP 签发（需要该设备的 Qualcomm KeyMint 支持），而不是复制 keybox。

---

## 14. 失败矩阵

| 场景 | 行为 |
|---|---|
| `age.pass` 缺失/权限不对 | 自动任务停止；报路径/权限；不猜密码 |
| Gist 直链 404 | 保留当前 keybox；提示重新导入/更新链接 |
| Gist 直链返回旧缓存 | 等待/重试；可用 API 读取（仍可不带 token，注意限流） |
| Gist token 401/403 | 发送端停止；提示更新 token |
| WebDAV 401/403 | 停止；提示更新应用密码 |
| WebDAV 409/412/423 | 重新读取 ETag；锁冲突下轮重试 |
| 公开 URL 401/403 | 说明需要认证；改用 WebDAV provider |
| 解密失败 | `decrypt_failed`；不覆盖本地，不清 state |
| CBOR/XML 非法 | 拒绝；保留当前 |
| seq 回退/同 seq 不同 hash | 拒绝；告警 |
| stream_id 不匹配 | 拒绝 |
| 叶子不足 `min_validity_days` | 拒绝；发送端重新 issue |
| 身份变化未允许 | 记录 `pending`；通知；不写目标 |
| OMK key retirement 触发 | 部署成功但可能删 key；UI 提前警告 |
| 目标写文件失败 | 保留旧文件；报 path/errno/SELinux |
| reload 失败 | 文件已落盘；标记 degraded；提示手动重启/查日志 |
| 禁用同步 | 停任务；保留已部署 keybox；不自动删除 |
| 跨设备 attestation 失败 | 本地无法可靠检测；UI 说明是使用风险，不是同步错误 |

---

## 15. 安全要求（MUST）

1. 密码/token/WebDAV 密码/keybox/私钥 **MUST NOT** 出现在 URL、二维码、日志、错误消息、WebUI 文本（输入框除外，提交后立即清空）、外部进程 argv、环境变量。
2. 所有 secret/state/staging **MUST** 为 `0600`；目录 `0700`；目标 keybox 权限按适配器要求。
3. 密码输入 **MUST** 走 stdin/TTY/`0600` 文件/§19.2 的 inbox；**MUST NOT** 使用 `--password <明文>`。
4. 远端响应 **MUST** 有大小上限；解密前不得无限缓冲。
5. age 解密失败 **MUST** 拒绝应用；不得“跳过校验”。
6. `seq/prev/stream_id/叶子到期` **MUST** 全部校验。
7. keybox XML 解析 **MUST** 禁用 DTD/外部实体；**MUST** 限制链长度/大小。
8. 私钥与叶子公钥 **MUST** 匹配；`DeviceID` **MUST** 与公钥推导一致。
9. 部署路径/owner/mode/reload **MUST** 只来自本地配置；远端 **MUST NOT** 指定。
10. 只允许 https；http **MUST** 需要显式 `--allow-insecure-http` 且仅本地调试。
11. URL **MUST** 拒绝 userinfo、未知字段、`..`、私网地址（默认）。
12. 写入 **MUST** 原子：同目录 temp + fsync + rename + fsync(dir)。
13. 部署失败 **MUST** 保留旧 keybox；不得先删除再写。
14. Gist token **MUST** 最小 scope；WebDAV 使用应用密码；可撤销。
15. `secret-file` 设备丢失/root 失守视为密码泄露；必须换密码 + 重新 issue keybox。
16. 首次启用 **MUST** 风险确认；默认关闭；不得静默开启。
17. 日志 **MUST** 只记 seq/hash/provider/路径/错误码，不记内容。

---

## 16. 实施阶段

| 阶段 | 内容 | 完成标准 |
|---|---|---|
| **P0** | `rkp-mini-sync-core`：CBOR envelope、age armor、URL parser、严格 keybox 校验、sync/deploy state；local directory transport | host 单测全过；无网络/无 Android 依赖 |
| **P1** | 独立 `rkp-mini-sync` 二进制；发送端 Gist（token）+ `push/share`；接收端 Gist 直链（无 token）+ `import/fetch`；OMK 手动部署 | 真实 Gist 创建/更新/拉取/解密；OMK reload；权限正确 |
| **P2** | WebDAV（自定义文件名、tmp+MOVE、If-Match）+ 公开 URL；TEESim/TrickyStore 适配器；deploy-state/identity gate | 坚果云实测；TEESim `config.json` 引用检查；身份变化拦截 |
| **P3** | `secret-file` 自动运行；发送端 `auto_issue_days` 阈值 + push；接收端定时 `run`；rotation policy `scheduled` | 重启后自动运行；OMK 警告；Doze 行为记录 |
| **P4** | 密码轮换、风险 UI/scope 文档、跨设备 QA、打包/版本、故障注入 | 验收清单全过；文档/AGENTS/README 更新 |

每个阶段都必须保持 `rkp-mini-issuer` 现有签发/status 契约不变；P0 先做 core，避免网络/密码代码污染签发路径。

---

## 17. 验收清单

### 17.1 单元/集成

- age passphrase 加解密 round-trip；错误密码、篡改、截断、超大输入全部拒绝。
- CBOR 字段/大小/编码；`prev`/`seq`/`stream_id` 校验。
- URL parser：拒绝 password/token/未知字段/userinfo/私网/`..`；接受三种 provider。
- keybox 校验：EC-only、链 >=2、key match、DeviceID 推导、DTD/XXE、超大 XML。
- state 原子写；断电模拟；staging 清理；sync/deploy 分离（`sync-state` 无 target/path）。
- Gist：secret gist 创建/更新；接收端无 token 直链读取；缓存/404/限流。
- WebDAV：自定义文件名；PUT+MOVE；If-Match；401/403；配额。
- 公开 URL：无认证读取；SSRF 拒绝；重定向限制。
- identity gate：same hash 幂等；scheduled 窗口；always/never；首次部署人工确认。
- 自动任务：secret-file 缺失/权限错；锁；重试；重启恢复。

### 17.2 安全

- 远端 bucket/gist/WebDAV 无明文 keybox/私钥。
- 日志/错误/URL/二维码无密码/token。
- `ps`/argv 无密码；临时文件 0600 且及时 unlink。
- `secret-file` 0600/目录 0700；SELinux 上下文正确。
- URL 解析拒绝 secret 字段；SSRF/XXE 测试通过。
- 禁用同步不会自动删除已部署 keybox。

### 17.3 设备

- OMK：写入后 watcher reload；owner `1017:1017`；`retire_stale_keybox_bound_keys` 警告可见。
- TEESim：`config.json` 引用匹配；watcher reload；EC-only 实测。
- 跨设备：首次风险确认；新设备部署后 attestation 行为记录；失败不伪报成功。
- 自动：重启后任务启动；Doze/省电下行为记录；手动 `run` 兜底。
- 密码轮换后旧密文/旧 keybox 行为记录。

---

## 18. 实现自由度与边界

已定决策不要改；以下是实现细节自由度：

- workspace vs 独立仓库：推荐 workspace；若工期紧可先独立 crate，但不要塞进 `rkp-mini-issuer` 单二进制。
- Gist 直链 vs API 解析：默认直链；API 作为无 token 回退。
- WebDAV MOVE vs If-Match：按目标服务器能力选择；保留失败回退。
- UI 技术栈：KernelSU WebUI（§19），复用 `webui/` 的 Vue + miuix-vue；CLI 保持同等能力。风险确认/禁用必须在两者都存在。
- 是否保留 staging 最近 1 份：允许，但必须 0600 且可清理。
- 是否加 Ed25519：v1 不做；未来如做，必须单独设计公钥分发，不要混入本方案。

## 19. WebUI 修订（2026-11-10，已定）

原 v1 只做 CLI，理由是“WebUI 经 `ksu.exec` 传密码会进 argv”。读 KernelSU manager 源码后该前提不成立，改为 WebUI 为主入口。

### 19.1 已确认事实（KernelSU `main`）

| 事实 | 来源 | 影响 |
|---|---|---|
| `exec(cmd[, opts], cb)` 用 libsu：`createRootShell` 构造 `libksud.so debug su [-g]`，命令经 `newJob().add(cmd)` 写入该 shell 的 **stdin** | `ui/webui/WebViewInterface.kt`、`ui/util/KsuCli.kt` | 命令文本不出现在任何进程 argv；只有 shell 再 `exec` 的**外部命令**会把参数放进 argv |
| `opts.env` 被拼成 `export k=v;` 前缀，同样走 stdin | 同上 | 不要用 env 传密钥：子进程会继承环境变量 |
| `Shell.enableVerboseLogging = BuildConfig.DEBUG` | `KsuCli.kt` | **debug 版** manager 会把命令写进 logcat；release 版不会。这是残余风险，写进风险说明 |
| WebView 同源 `https://mui.kernelsu.org`，生产 CSP `connect-src 'none'` | `docs/WEBUI_DEVELOP.md` | 页面拿到的密码无法外传；只能经 `exec` 落盘 |

### 19.2 密钥写入（inbox）

```text
WebUI: hex = utf8ToHex(password)        // [0-9a-f]，无引号/转义/换行问题
exec:  umask 077; mkdir -p <inbox> 2>/dev/null; echo <hex> > <inbox>/<kind>.hex && \
       <bin> --json secret set <kind> --from-file <inbox>/<kind>.hex --hex
```

- 只用 shell **内建**（`umask`、`echo`、重定向）承载密钥；`mkdir`、`rkp-mini-sync` 等外部命令的 argv 只有路径。hex 让 mksh/ash/bash 的 `echo` 行为一致；不用 here-doc（mksh 会落临时文件）。
- inbox：发送端 `<数据根>/sync/inbox/`，接收端 `/data/adb/rkp_sync/sync/inbox/`，`0700`。
- `secret set --from-file`：`O_NOFOLLOW` 打开；必须是当前 euid 拥有的普通文件、无 group/other 位；≤ 8 KiB；`--hex` 解码（容忍一个行尾换行）；非空 UTF-8；原子写入配置的 secret 路径（0600）。**无论成败都删除 inbox 文件**，并清空内存。
- 同一机制用于 `rotate-password --from-file <f> --hex`（新口令）。
- 页面：`type=password`、`autocomplete=off`；提交后立即清空输入与 hex 变量；从不回显、从不读取已存的 secret 文件，只展示“已设置/缺失/权限错误”。

### 19.3 非交互确认（绑定版本）

| 操作 | CLI | 绑定 |
|---|---|---|
| 风险接受 | `rkp-mini-sync --json risk` 输出 `{version, items[], passphrase_path}`；`enable --accept-risk <version>` | `<version>` 必须等于当前 `RISK_VERSION`；旧 UI 不能替新版本说明“同意” |
| 首次导入 TOFU | `fetch` 失败 `first_import_needs_confirmation`，错误信封带 `data`（seq/issued_at/device_id/leaf_not_after/keybox_sha256/plaintext_sha256）；`fetch --accept-first <plaintext_sha256>` | 只接受与展示一致的那一版；否则 `confirmation_mismatch` |
| 首次部署 / 身份变化 | `rkp-mini-deployer plan --json`（只读：gate、warnings、targets、old/new）；`rkp-mini-deployer apply --confirm-first <keybox_sha256>` / `--allow-identity-change <keybox_sha256>` | 在 `sync.lock` 内比对 staged sha；后台循环若已换了 staging，拒绝 `confirmation_mismatch` |

sha 参数接受完整 hex 或 ≥16 位前缀。TTY 交互路径不变（仍可 y/N）。

### 19.4 配置编辑

- `rkp-mini-sync --json config show`：生效配置（不含任何密钥值，只含 secret 路径与状态）。
- `rkp-mini-sync config set <key> <value>`：**白名单**，写后重新加载校验，失败回滚。发送端：`provider`、`label`、`min_validity_days`、`request_timeout_seconds`、`proxy`、`gist_id`、`gist_filename`、`webdav_endpoint`、`webdav_path`、`webdav_user`、`password_mode`、顶层 `auto_issue_days`。接收端：`label`、`min_validity_days`、`request_timeout_seconds`、`proxy`、`password_mode`、`identity_change_policy`、`rotation_lead_days`、`auto_deploy`。位置字段（接收端）只经 `import` 写入；`enabled`/`risk_ack_*` 只经 `enable/disable`；部署目标只经 `rkp-mini-deployer add-target/remove-target`。
- `rkp-mini-issuer` WebUI 原“配置只读”决策仅对 `rkp-mini-issuer` 自身字段保持；`[sync]` 与 `auto_issue_days` 经上面的命令修改。

### 19.5 页面

- **发送端**（`rkp_mini_issuer` WebUI 新增“远程同步”卡片 + 底部弹层）：状态（启用、provider、位置、seq、上次推送/错误、本地 keybox 到期、自动循环是否运行）；初始化；provider 配置；设置 age 口令 / Gist token / WebDAV 密码；风险确认弹层（§13.2 每条独立复选框，默认全不勾，全勾才可提交，含 §19.1 debug manager 残余风险）；启用/禁用；立即推送；分享（URI + 由 `share --json` 的 `qr` 矩阵本地渲染的二维码 + “不含密码”提示）；轮换口令；清除本地数据；同步日志。
- **接收端**（`rkp_mini_deployer` 模块 `webroot/`，同一 Vue 工程的第二个入口）：粘贴导入链接 → `import --check` 预览位置 → 确认后 `import --yes`；风险确认；口令/WebDAV 密码；目标增删（omk/teesim/tricky_store）；策略设置；拉取（TOFU 弹层展示事实后 `--accept-first <sha>`）；部署（先 `rkp-mini-deployer plan`，首次/身份变化弹层展示 old→new、OMK key 删除警告、TEESim re-attest、“未做根信任检查”，再带 sha 执行）；pending 提示；立即运行；禁用；清除；移除已部署 keybox（两步确认）；日志。
- “部署目标”弹层先调用 `rkp-mini-deployer targets`：只有 `module.installed` 为 true 的内置目标可以添加；未检测到模块的目标显示为不可选并给出模块 id；检测失败时不提供任何添加入口。已停用的模块仍可选择，但会提示启用前不会重载。
- 长任务（push、fetch、rotate-password）走 `webroot/runner/sync.sh <op> [非密钥参数]`，与 `issue.sh` 相同的 `setsid` + 状态文件 + 轮询模型；短命令（status/plan/config/secret/enable）直接 `exec`。
- 启用后页面立即以 `setsid` 启动模块的 `sync-loop.sh`（脚本自带 pid 锁，重复启动无害）；禁用后循环在下一次唤醒时退出。
- 页面从不读取或展示 keybox、私钥、口令、token；`staging/`、`secrets/`、`inbox/` 不出现在任何 `cat`/`ls` 命令里。

---

## 附录 A：配置示例

### A.1 发送端（节选）

```toml
[sync]
enabled = false
risk_ack_version = 0
provider = "gist"
stream_id = "st_01J..."
min_validity_days = 14
request_timeout_seconds = 30

gist_id = ""
gist_filename = "rkp-sync.age"
gist_url = ""
gist_token_file = "/data/misc/keystore/rkp-mini/sync/gist.token"

webdav_endpoint = ""
webdav_path = "/rkp/rkp-sync.age"
webdav_user = ""
webdav_password_file = "/data/misc/keystore/rkp-mini/sync/webdav.pass"

password_mode = "secret-file"
password_file = "/data/misc/keystore/rkp-mini/sync/age.pass"
```

`auto_issue_days` 保持在主配置顶层；语义改为“剩余天数阈值”。

### A.2 接收端（节选）

```toml
[sync]
enabled = false
risk_ack_version = 0
provider = "gist"
stream_id = "st_01J..."
min_validity_days = 7
request_timeout_seconds = 30

gist_url = "https://gist.githubusercontent.com/<owner>/<id>/raw/rkp-sync.age"
gist_id = ""
gist_filename = "rkp-sync.age"

webdav_endpoint = ""
webdav_path = ""
webdav_user = ""
webdav_password_file = "/data/adb/rkp_sync/secrets/webdav.pass"

password_mode = "secret-file"
password_file = "/data/adb/rkp_sync/secrets/age.pass"

[deploy]
identity_change_policy = "scheduled"
rotation_lead_days = 30
auto_deploy = true

[[deploy.targets]]
id = "omk"
path = "/data/misc/keystore/omk/keybox.xml"
owner = "1017:1017"
mode = "0600"
reload = "auto"
```

## 附录 B：CLI JSON 契约

统一输出：

```json
{"ok":true,"data":{...}}
{"ok":false,"error":{"stage":"remote|decrypt|validate|state|deploy|config|generic","code":"...","message":"..."}}
```

- stdout 只输出一个 envelope；诊断写 stderr。
- 退出码：
  - `0` 成功
  - `2` 用法/配置/风险确认缺失
  - `8` 同步/网络/解密/校验/state 失败
  - `9` 部署/写入/reload 失败
  - `1` 其它

`push --json` data：

```json
{
  "provider": "gist",
  "stream_id": "st_...",
  "seq": 42,
  "ciphertext_sha256": "...",
  "url": "https://gist.githubusercontent.com/.../raw/rkp-sync.age",
  "uploaded_at": 1762750000
}
```

`fetch --json` data：

```json
{
  "provider": "gist",
  "stream_id": "st_...",
  "seq": 42,
  "plaintext_sha256": "...",
  "keybox_sha256": "...",
  "device_id": "rkp-mini-...",
  "leaf_not_after": 1794000000,
  "staged_path": "/data/adb/rkp_sync/staging/<sha>.xml",
  "previous_seq": 41
}
```

`rkp-mini-deployer apply --json` data：

```json
{
  "keybox_sha256": "...",
  "seq": 42,
  "targets": [
    {
      "id": "omk",
      "path": "/data/misc/keystore/omk/keybox.xml",
      "ok": true,
      "sha256": "...",
      "reload": "auto",
      "reload_status": "ok",
      "error": null
    }
  ]
}
```

## 附录 C：参考

- age：<https://age-encryption.org/>
- GitHub Gist API：<https://docs.github.com/en/rest/gists>
- WebDAV（RFC 4918）：<https://www.rfc-editor.org/rfc/rfc4918>
- OhMyKeymint：<https://github.com/qwq233/OhMyKeymint>
- TEESimulator：<https://github.com/JingMatrix/TEESimulator>
- TrickyStore：<https://github.com/5ec1cff/TrickyStore>
- 现有 WebUI 方案：`docs/WEBUI_DEVELOP.md`
- 仓库约束：`AGENTS.md`

---

## 附录 D：实现前检查

实现 Agent 在写代码前应确认：

- [ ] 已读 `AGENTS.md` 和本文；没有重新引入服务端。
- [ ] `sync-state` 不出现 target/path/owner/reload。
- [ ] URL/二维码/日志中没有密码/token。
- [ ] Gist 接收端默认走直链，不要求 token。
- [ ] `rkp-mini-sync` 独立构建；`rkp-mini-issuer` 不链接 sync 依赖。
- [ ] 首次启用风险确认、默认关闭、可禁用。
- [ ] OMK `retire_stale_keybox_bound_keys` 警告已实现。
- [ ] TEESim `config.json` keybox 引用检查已实现。
- [ ] 自动运行使用 `secret-file` 时，权限/缺失/轮换行为已覆盖。
- [ ] 跨设备风险文案已写入 UI 和 README/AGENTS。
