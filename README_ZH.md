<div align="center">

# RKP-Mini

**在高通设备上签发 Android RKP keybox，并可选择加密同步到另一台设备。**

[![Build](https://img.shields.io/github/actions/workflow/status/etnperlong/rkp-mini/release.yml?branch=main&style=flat-square&label=build)](https://github.com/etnperlong/rkp-mini/actions/workflows/release.yml)
[![Release](https://img.shields.io/github/v/release/etnperlong/rkp-mini?include_prereleases&style=flat-square&label=release)](https://github.com/etnperlong/rkp-mini/releases)
![Status](https://img.shields.io/badge/status-experimental-orange?style=flat-square)
![Platform](https://img.shields.io/badge/Android-arm64-3DDC84?style=flat-square&logo=android&logoColor=white)
![KernelSU](https://img.shields.io/badge/KernelSU-module-4B5563?style=flat-square)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)

[English](README.md) | **简体中文**

</div>

RKP-Mini 是一对自带 WebUI 的 KernelSU 模块。**签发端（Issuer）** 生成全新的 P-256 密钥，
让本机的高通 KeyMint TA（Native V2，厂商命令 `0x3122`）为其签署 CSR，再向 Google RKP 服务
申请证书，最后写出兼容 TrickyStore 的 `keybox.xml`。**部署端（Deployer）** 在另一台设备上
接收并安装这份 keybox。

> [!WARNING]
> - **实验性项目。** 针对一台高通参考设备（arm64）开发；其他设备与固件均未经测试，且尚无真机签发成功记录。
> - **仅支持 Native V2。** 固件不支持时直接报错：不回退到 V1/AIDL，不伪装 TEE 或启动状态，也不保证通过 Play Integrity。
> - **远程同步默认关闭。** 在另一台设备上使用 keybox 属于 keybox 克隆/伪装，并非合法的 RKP provisioning；启用前必须逐项确认风险。

## 模块

| 模块 | 安装位置 | 作用 |
|---|---|---|
| **RKP-Mini Issuer**（`rkp_mini_issuer`） | 负责签发的高通设备 | 签发并续期 keybox；可选择加密后推送到你自己的 GitHub Gist 或 WebDAV |
| **RKP-Mini Deployer**（`rkp_mini_deployer`） | 接收 keybox 的设备 | 拉取、解密并校验 keybox，然后写入 OhMyKeymint、TEESimulator、TrickyStore / TEESimulator-RS 或自定义路径 |

WebUI 支持简体中文、繁體中文、English、日本語、한국어、Deutsch、Français 和 Italiano。

## 环境要求

- 已 root 的 arm64 设备，使用 KernelSU。
- 仅签发端：高通平台，存在 `/vendor/lib64/libkeymasterdeviceutils.so` 和 `/dev/smcinvoke`，并且可以联网。
- 仅部署端：至少安装上表中的一个目标模块。

## 安装与使用

1. 在 [Releases](https://github.com/etnperlong/rkp-mini/releases) 下载对应用途的 ZIP。
   可选校验：`sha256sum --ignore-missing -c SHA256SUMS`。
2. 在 KernelSU 管理器中安装，然后重启。
3. 点击模块卡片上的 **打开**。

### 签发 keybox

1. **状态** 页：确认各项环境检查全部通过。
2. **签发** 页 → **立即签发**。签发在后台运行，可以关闭页面，稍后回来查看结果。
3. keybox 写入 `/data/misc/keystore/rkp-mini/output/keybox.xml`。自行复制到你的 keystore 模块，或交给部署端处理（见下文）。

高通 RKP 证书有效期只有约 12–14 天。在 **签发 → 签发设置** 中开启自动签发（建议 3 天），
模块会在后台自动续期。

### 同步到另一台设备（可选）

1. **发送端**（签发端 → **同步** 页）：选择 Gist 或 WebDAV，设置加密密码以及 Gist token /
   WebDAV 应用密码，逐项勾选风险确认，推送，然后分享链接或二维码。
2. **接收端**（部署端）：导入链接，输入加密密码（请通过另一条渠道发送），添加部署目标，
   点击 **立即部署**，并确认页面显示的首个版本。

之后两端会自动刷新：签发端约每 12 小时一次，部署端约每 8 小时一次。

- 你的存储中始终只有密文；链接和二维码只包含存储位置，不包含密码。
- 关闭同步或删除远端文件**不会**撤销已被接收的 keybox。需要撤销时，请修改加密密码**并**重新签发。
- 接收端只校验结构、密钥匹配和有效期，不验证证书签名、根信任或吊销状态。

## 故障排查

签发失败不会替换现有的 keybox。结果页会显示失败阶段、错误码和 `ta_status`；完整日志在
**更多 → 日志**。

| 错误 | 含义 |
|---|---|
| `-100` UNIMPLEMENTED | KeyMint TA 没有 `0x3122` 命令 |
| `-18` INCOMPATIBLE_KEY_FORMAT | TA 需要 MacedPublicKey，Native V2 不可用 |
| `-40` INVALID_TAG | 厂商命令或帧格式不匹配 |
| HTTP `444` | Google RKP 服务不认可该设备 |

出现这些错误说明固件不受支持，RKP-Mini 不会改用其他协议。

## 文档

- [使用指南](docs/USAGE_ZH.md)：命令行、自动任务、撤销，以及从旧版 `rkp_mini` / `rkp_sync` 模块迁移
- [开发文档](docs/DEVELOPMENT_ZH.md)：构建、测试与发布
- 设计记录：[远程同步](docs/REMOTE_SYNC_DEVELOP_ZH.md)、[WebUI](docs/WEBUI_DEVELOP_ZH.md)

## 致谢

- [Duck ToolBox](https://github.com/eltavine/Duck-ToolBox)：本项目的签发流程以其设备端 RKP 签发器为蓝本。
- [TeeForge-CD](https://github.com/mcxiaochenn/TeeForge-CD)：它对 Duck ToolBox 签发方案的精简思路是本项目保持精简的参考。

## 许可证

[MIT](LICENSE) © 2026 Evans Mike
