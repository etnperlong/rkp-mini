# Development

**English** | [简体中文](DEVELOPMENT_ZH.md)

Architecture, conventions and contracts are in [`AGENTS.md`](../AGENTS.md). Read the design
record before changing an area: [`REMOTE_SYNC_DEVELOP.md`](REMOTE_SYNC_DEVELOP.md) for
sync/deployer, [`WEBUI_DEVELOP.md`](WEBUI_DEVELOP.md) for `webui/`. Both are translations;
their Chinese originals (`*_ZH.md`) are authoritative.

## Build

Needs x86_64 Linux, the Android NDK, `rustup target add aarch64-linux-android`,
[Bun](https://bun.sh), `zip`, `unzip`, `file`, `readelf` and `sha256sum`.

```sh
NDK=/path/to/android-ndk ./build.sh   # without NDK: highest version under ~/Android/Sdk/ndk/
```

Produces `dist/rkp_mini_issuer-v<version>.zip` and `dist/rkp_mini_deployer-v<version>.zip`,
both installable from the KernelSU manager. The script also checks the ELF architecture, each
ZIP's file list, that both ZIPs ship a byte-identical `rkp-mini-sync`, and that each
`module.prop` version matches its crate.

`build.sh` remaps the checkout, the cargo registry and the rustup toolchain to neutral
prefixes for rustc and the C compiler, so the shipped binaries carry no local build
paths.

## Test

```sh
cargo fmt --all --check
cargo test --locked
cargo clippy --workspace --locked --all-targets -- -D warnings
python3 -m unittest discover -s .github/scripts -p 'test_*.py' -v
cd webui && bun install --frozen-lockfile && bun run build   # type-check + both pages
cd webui && bun run dev:issuer                               # or dev:deployer: browser preview on fixtures
```

Host tests do not establish device issuance, installation, SELinux behaviour or hot reload.

## Releases

`.github/workflows/release.yml` runs the checks above and `./build.sh` on every push to
`main` and every `v*` tag, then publishes with the workflow's `GITHUB_TOKEN`.

| Trigger | Version and publication |
|---|---|
| Push to `main` | `<source version>-nightly.<UTC date>.<run number>.g<short SHA>`, e.g. `0.4.0-nightly.20261113.42.gabcdef0`; replaces the rolling **Pre-release** and fixed `nightly` tag, never Latest |
| Push tag `vX.Y.Z` | The tag version; a normal release marked **Latest** |

```sh
git tag v0.5.0
git push origin v0.5.0
```

- Only canonical tags are accepted (`v0.5.0`, not `v01.5.0` or `v0.5.0-rc1`). The tag wins
  over the checked-in versions.
- In the CI checkout, `.github/scripts/prepare_release.py` stamps one version into all six
  crates, `Cargo.lock` and both `module.prop` files, and sets `versionCode` to the build's
  Unix timestamp. Nothing is committed back. Nightlies require the checked-in issuer and
  deployer versions to match.
- Each release carries both ZIPs and `SHA256SUMS`; the same files are kept as an Actions
  artifact for 30 days. A successful nightly build replaces the previous nightly release,
  assets and tag; stable releases are never overwritten. Same-ref workflow runs are serialized.
- Nightly replacement deletes the old release and tag before publishing the new build, so
  downloads may be briefly unavailable; a publish failure can leave nightly absent until
  the next successful publish. Existing versioned nightly tags/releases are not removed automatically.
- Release notes are bilingual and written inline in the workflow. They link to
  `README.md#install--use`, so keep that heading.

Dependabot (`.github/dependabot.yml`) opens daily grouped PRs for Actions, Cargo and the
WebUI's Bun packages. Majors for Rust and the WebUI stay separate, `miuix-vue` is excluded
to keep its exact pin, and nothing is auto-merged.

## Not implemented

- Ed25519-signed manifests (v1 relies on the age passphrase alone).
