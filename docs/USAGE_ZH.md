# 使用指南

[English](USAGE.md) | **简体中文**

[README](../README_ZH.md) 之外的内容：命令行、后台自动任务、撤销与迁移。本文所有命令都在设备的
root shell 中执行。协议与威胁模型见 [`REMOTE_SYNC_DEVELOP_ZH.md`](REMOTE_SYNC_DEVELOP_ZH.md)。

## 命令

三个相互独立的二进制，彼此从不调用。

| 命令 | 职责 | 所在模块 |
|---|---|---|
| `rkp-mini-issuer` | 通过 KeyMint TA 获取 keybox，查看本地状态 | 签发端 |
| `rkp-mini-sync` | 把 keybox 推送到加密的远端存储，或从中拉取 | 签发端、部署端 |
| `rkp-mini-deployer` | 把已校验并暂存的 keybox 写入本地目标 | 部署端 |

二进制位于 `/data/adb/modules/<模块 id>/bin/`。大多数命令支持 `--json`，输出机器可读的结果。

### 签发端

```sh
ISSUER=/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-issuer
$ISSUER status                          # 本地环境与 keybox 检查（不联网，不调用 TA）
$ISSUER issue --acknowledge             # 真实签发：新密钥、联网、调用 TA
$ISSUER config set auto_issue_days 3    # 剩余 ≤ 3 天时自动签发；0 为关闭
$ISSUER remove --yes                    # 删除已签发的 keybox、私钥、证书链和元数据
```

输出目录：`/data/misc/keystore/rkp-mini/output/`（`keybox.xml`、`cert-chain.pem`、`cert-chain.der`、
`private-key.pem`、`meta.json`）。日志：`/data/misc/keystore/rkp-mini/logs/issue-<unix>.log`。

### 同步：发送端

`SYNC` = `/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-sync`

```sh
SYNC --role sender --config /data/misc/keystore/rkp-mini/config.toml init --provider gist
SYNC --role sender secret set age            # 从终端读取加密密码（不回显），以 0600 保存
SYNC --role sender secret set gist-token     # 或：secret set webdav-password
SYNC --role sender enable                    # 显示风险说明，输入 I ACCEPT
                                             # （非交互：先用 `risk` 查看，再执行
                                             #   `enable --accept-risk <version>`）
SYNC --role sender push                      # 首次推送会创建 secret gist
SYNC --role sender share                     # 导入链接 + 二维码（不含密码）
```

Gist token 是只勾选 `gist` 权限的 GitHub PAT。WebDAV 请使用可单独撤销的应用密码。

### 同步与部署：接收端

`SYNC` = `/data/adb/modules/rkp_mini_deployer/bin/rkp-mini-sync`，
`DEPLOY` = `/data/adb/modules/rkp_mini_deployer/bin/rkp-mini-deployer`
（两者都需追加 `--config /data/adb/rkp_sync/config.toml --role receiver`）：

```sh
SYNC import 'rkpsync://v1/import?p=…'         # 核对存储位置，确认风险，输入加密密码
DEPLOY targets                               # 查看已安装哪些目标模块
DEPLOY add-target omk                        # 或 teesim / tricky_store
SYNC fetch                                   # 首个版本：首次使用信任（TOFU）确认
                                             # （非交互：--accept-first <plaintext_sha256>）
DEPLOY plan                                  # 预览
DEPLOY apply --confirm-first <keybox_sha256> # 轮换时改用 --allow-identity-change
```

其他命令：`check-remote`（两种角色均可；只读的更新检查）、`rotate-password` 和
`remote-delete`（发送端）、`remove-target` 和 `remove`（部署端）。

## 部署目标

| 目标 | 路径 | 说明 |
|---|---|---|
| `omk`（OhMyKeymint） | `/data/misc/keystore/omk/keybox.xml` | 若 OMK 自身的扫描器会拒绝该 keybox，则以 `omk_incompatible` 跳过 |
| `teesim`（TEESimulator） | `/data/adb/teesim/` 下的某个 `.xml` 文件 | 必须被 TEESimulator 的 `config.json` 引用 |
| `tricky_store`（TrickyStore、TEESimulator-RS） | `/data/adb/tricky_store/keybox.xml` | |
| 自定义 | 你配置的任意路径 | 需主动启用 |

WebUI 只在对应模块已安装时才提供内置目标。这些模块会监视自己的 keybox 文件，替换后应当无需重启即可
生效；如果新 keybox 没有生效，请重启。各工具自身的配置（OMK 的 `config.toml` / `injector.toml`、
TEESimulator 的 `config.json`）从不参与同步。

## 后台自动任务

- **签发端**：约每 12 小时一轮，当前 keybox 剩余天数 ≤ `auto_issue_days` 时签发新的 keybox；若已启用
  同步，随后推送。该值应保持较小：证书有效期只有约 12–14 天，设为 12 或更大会导致每一轮都重新签发。
- **部署端**：启用同步后约每 8 小时一轮，拉取并部署。
- 模块卡片上的 **Action** 按钮：立即执行一轮上述任务并显示日志。它不是强制签发；自动签发关闭时，
  签发端这一轮会跳过签发（请使用 **立即签发**）。
- Doze 和厂商省电策略可能推迟这些循环。日志：各数据目录下的 `logs/auto.log`（签发端）和
  `logs/sync.log`（部署端），也可在 WebUI → **更多 → 日志** 查看。

WebUI 的 **立即部署** 先拉取，再应用已暂存的内容，因此即使拉取失败，也能用此前已校验的 keybox 修复
损坏的目标。首次部署，或不符合策略的身份变更，会保持待确认状态，直到你批准。

身份变更遵循 `[deploy].identity_change_policy`。默认值 `scheduled` 只有在旧证书到期前
`rotation_lead_days`（默认 4）天内、并且新证书比旧证书更晚到期时，才自动部署新的 keybox。OMK 的
`retire_stale_keybox_bound_keys`（默认 true）会删除绑定在被替换 keybox 上的客户端密钥。

## 语言

模块卡片说明和 Action 输出跟随 WebUI → **更多 → 语言** 中选择的语言；刷新 KernelSU 模块列表即可
看到变化。首次打开 WebUI 之前为简体中文。原始诊断信息保持其原有语言。

## 密钥与密码

在 WebUI 中输入的密码会写入一个 `0600` 的收件文件，由 `rkp-mini-sync` 读取后删除；它们不会出现在
进程参数、URL 或日志中。*debug* 版的 KernelSU 管理器会把 shell 命令写入 logcat：请使用 release 版。

## 停用、删除与撤销

| 目的 | 命令 |
|---|---|
| 停止同步，保留已部署的 keybox | `sync disable` |
| 只停止自动推送（发送端） | `sync config set auto_push false` |
| 删除本地密钥与状态 | `sync forget`（发送端加 `--reset` 还会移除 `[sync]` 表） |
| 删除远端文件（整个 Gist） | `sync remote-delete --yes` |
| 删除已部署的 keybox | `deployer remove --yes` |

卸载模块会保留其数据目录（`/data/misc/keystore/rkp-mini`、`/data/adb/rkp_sync`）。

以上操作都不是撤销：接收端会保留已拉取的内容，旧的 Gist 修订版本仍可用旧密码解密。需要撤销时，
请执行 `sync rotate-password` **并**签发新的 keybox。任一端设备丢失或 root 权限被攻破，都应视为
密码与私钥已经泄露。

## 从旧的单模块布局迁移

旧的 `rkp_mini`（发送端）和 `rkp_sync`（接收端）模块使用相同的数据目录；同一角色的旧模块仍存在时，
新模块的安装程序会拒绝安装。

1. 备份 `/data/adb/rkp_sync`：旧接收端的卸载脚本会删除它。
2. 停用并卸载旧模块。
3. 如有需要先恢复备份，再安装新模块。
