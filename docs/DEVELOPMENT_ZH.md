# 开发文档

[English](DEVELOPMENT.md) | **简体中文**

架构、约定与各项契约见 [`AGENTS.md`](../AGENTS.md)（英文）。修改某个部分之前请先阅读对应的设计
记录：同步与部署端见 [`REMOTE_SYNC_DEVELOP_ZH.md`](REMOTE_SYNC_DEVELOP_ZH.md)，`webui/` 见
[`WEBUI_DEVELOP_ZH.md`](WEBUI_DEVELOP_ZH.md)。

## 构建

需要 x86_64 Linux、Android NDK、`rustup target add aarch64-linux-android`、
[Bun](https://bun.sh)，以及 `zip`、`unzip`、`file`、`readelf` 和 `sha256sum`。

```sh
NDK=/path/to/android-ndk ./build.sh   # 未设置 NDK 时使用 ~/Android/Sdk/ndk/ 下版本最高的目录
```

生成 `dist/rkp_mini_issuer-v<version>.zip` 和 `dist/rkp_mini_deployer-v<version>.zip`，两者都可
在 KernelSU 管理器中安装。脚本还会检查 ELF 架构、每个 ZIP 的文件清单、两个 ZIP 中的
`rkp-mini-sync` 是否逐字节相同，以及每个 `module.prop` 的版本是否与对应 crate 一致。

`build.sh` 会通过 `--remap-path-prefix` 与 `-ffile-prefix-map` 把检出目录、cargo 与 rustup
工具链路径映射为中性前缀，产物中不含本机构建路径。

## 测试

```sh
cargo fmt --all --check
cargo test --locked
cargo clippy --workspace --locked --all-targets -- -D warnings
python3 -m unittest discover -s .github/scripts -p 'test_*.py' -v
cd webui && bun install --frozen-lockfile && bun run build   # 类型检查 + 两个页面
cd webui && bun run dev:issuer                               # 或 dev:deployer：基于模拟数据的浏览器预览
```

主机测试不能证明真机签发、安装、SELinux 行为或热重载是否正常。

## 发布

`.github/workflows/release.yml` 在每次推送到 `main` 和每次推送 `v*` 标签时运行上述检查与
`./build.sh`，然后使用 workflow 自带的 `GITHUB_TOKEN` 发布。

| 触发条件 | 版本与发布方式 |
|---|---|
| 推送到 `main` | `<源码版本>-nightly.<UTC 日期>.<运行序号>.g<短 SHA>`，例如 `0.4.0-nightly.20261113.42.gabcdef0`；每次运行单独发布一个 **Pre-release**，不会标记为 Latest |
| 推送标签 `vX.Y.Z` | 以标签为版本；正式发布，标记为 **Latest** |

```sh
git tag v0.5.0
git push origin v0.5.0
```

- 只接受规范的标签（`v0.5.0`，而非 `v01.5.0` 或 `v0.5.0-rc1`）。标签优先于仓库中已提交的版本号。
- 在 CI 的检出目录中，`.github/scripts/prepare_release.py` 把同一个版本号写入全部六个 crate、
  `Cargo.lock` 和两个 `module.prop`，并把 `versionCode` 设为构建时的 Unix 时间戳。这些改动不会
  提交回仓库。nightly 要求仓库中签发端与部署端的版本号一致。
- 每个 release 包含两个 ZIP 和 `SHA256SUMS`；同样的文件还会作为 Actions artifact 保留 30 天。
  重新运行不会覆盖已发布的 release。
- 发布说明为中英双语，直接写在 workflow 里。其中链接到 `README.md#install--use`，因此请保留
  该标题。

Dependabot（`.github/dependabot.yml`）每天为 Actions、Cargo 和 WebUI 的 Bun 依赖创建分组 PR。
Rust 与 WebUI 的大版本更新单独成 PR，`miuix-vue` 被排除以保持其精确版本，任何更新都不会自动合并。

## 尚未实现

- 使用 Ed25519 签名的 manifest（v1 仅依赖 age 密码）。
